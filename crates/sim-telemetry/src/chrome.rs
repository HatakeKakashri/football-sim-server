//! Deterministic Chrome/Perfetto JSON trace layer.
//!
//! Only spans/events whose `target` is [`crate::emit::TRACE_TARGET`] are
//! written. Timestamps are simulation time (`tick * 1e6 / 60` µs plus a
//! per-tick sequence number), never wall-clock, so output is reproducible.

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Number, Value, json};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::emit::TRACE_TARGET;
use crate::metadata::TraceMetadata;

const MICROS_PER_SECOND: u64 = 1_000_000;
const TICKS_PER_SECOND: u64 = 60;
/// Highest per-tick sequence offset. One tick spans at least
/// `MICROS_PER_SECOND / TICKS_PER_SECOND` µs (16 666), so capping the offset
/// one below that keeps a tick's events inside its own time slot.
const MAX_SEQ: u64 = MICROS_PER_SECOND / TICKS_PER_SECOND - 1;

/// Layer that serialises `sim_trace` spans/events as a Chrome trace array.
pub struct ChromeTraceLayer<W: Write + Send + 'static> {
    shared: Arc<Mutex<State<W>>>,
}

struct State<W> {
    out: W,
    first: bool,
    finished: bool,
    error: Option<std::io::Error>,
    seen_processes: BTreeSet<u64>,
    seen_threads: BTreeSet<(u64, u64)>,
    last_tick: Option<u64>,
    seq: u64,
    metadata: Option<TraceMetadata>,
}

/// Closes the JSON array and flushes when dropped or `finish`ed.
pub struct TraceGuard {
    finisher: Arc<dyn Finish>,
}

trait Finish: Send + Sync {
    fn finish(&self) -> std::io::Result<()>;
}

impl<W: Write + Send> Finish for Mutex<State<W>> {
    fn finish(&self) -> std::io::Result<()> {
        let mut state = lock(self);
        if state.finished {
            return Ok(());
        }
        state.finished = true;
        if let Some(err) = state.error.take() {
            return Err(err);
        }
        let closing = if state.first { "[\n]\n" } else { "\n]\n" };
        state.out.write_all(closing.as_bytes())?;
        state.out.flush()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .expect("invariant: trace writer lock is never held across a panic")
}

impl TraceGuard {
    /// Finish the trace and report any I/O error.
    ///
    /// # Errors
    ///
    /// Propagates the first write/flush failure.
    pub fn finish(self) -> std::io::Result<()> {
        self.finisher.finish()
    }
}

impl Drop for TraceGuard {
    fn drop(&mut self) {
        // Best effort only: callers who care about the result use `finish`.
        let _ = self.finisher.finish();
    }
}

impl<W: Write + Send + 'static> ChromeTraceLayer<W> {
    /// Create a layer writing to `out`, plus the guard that finalises it.
    #[must_use]
    pub fn new(out: W) -> (Self, TraceGuard) {
        Self::with_metadata(out, None)
    }

    /// Create a layer that stamps a `process_labels` metadata event at startup
    /// carrying the run's `displayTimeUnit`, seed, interval, and full range.
    ///
    /// The metadata event fixes the
    /// [microsecond-vs-millisecond rendering bug](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU/preview)
    /// (Perfetto defaults to ms; we write µs) and lets a developer opening a
    /// trace 6 months from now see what they ran.
    #[must_use]
    pub fn with_metadata(out: W, metadata: Option<TraceMetadata>) -> (Self, TraceGuard) {
        let shared = Arc::new(Mutex::new(State {
            out,
            first: true,
            finished: false,
            error: None,
            seen_processes: BTreeSet::new(),
            seen_threads: BTreeSet::new(),
            last_tick: None,
            seq: 0,
            metadata,
        }));
        (
            Self {
                shared: Arc::clone(&shared),
            },
            TraceGuard { finisher: shared },
        )
    }
}

/// Build the run-header `M` event. Carries `displayTimeUnit` so Perfetto
/// renders the trace in microseconds (we write µs) rather than its default
/// of milliseconds. Also carries run identity so a developer opening the
/// file 6 months from now knows what they ran.
fn build_metadata_event(metadata: &TraceMetadata) -> Value {
    let mut args = Map::new();
    args.insert("displayTimeUnit".into(), Value::String("us".into()));
    args.insert("seed".into(), metadata.seed().into());
    args.insert("interval_ticks".into(), metadata.interval_ticks().into());
    if let Some((start, end)) = metadata.full_range() {
        args.insert("full_range".into(), json!([start, end]));
    }
    json!({
        "name": "process_labels",
        "ph": "M",
        "pid": 0,
        "tid": 0,
        "args": Value::Object(args),
    })
}

impl<W: Write> State<W> {
    fn put(&mut self, value: &Value) {
        if self.error.is_some() || self.finished {
            return;
        }
        let header = self.metadata.take().map(|m| build_metadata_event(&m));
        let separator = if self.first { "[\n" } else { ",\n" };
        self.first = false;
        let mut result: std::io::Result<()> = Ok(());
        if let Some(ref header) = header {
            // Emit header first (with its own separator), then a comma, then
            // the value. This keeps Perfetto's "[\n" prefix even when the
            // metadata event lands before any process_name.
            result = result
                .and_then(|()| self.out.write_all(separator.as_bytes()))
                .and_then(|()| {
                    serde_json::to_writer(&mut self.out, header).map_err(Into::into)
                })
                .and_then(|()| self.out.write_all(b",\n"))
                .and_then(|()| serde_json::to_writer(&mut self.out, value).map_err(Into::into));
        } else {
            result = self
                .out
                .write_all(separator.as_bytes())
                .and_then(|()| serde_json::to_writer(&mut self.out, value).map_err(Into::into));
        }
        if let Err(err) = result {
            self.error = Some(err);
        }
    }

    fn next_ts(&mut self, tick: u64) -> u64 {
        if self.last_tick == Some(tick) {
            // Saturate: events beyond the slot share the last timestamp (the
            // file order still nests them correctly) instead of leaking into
            // the next tick's range.
            self.seq = (self.seq + 1).min(MAX_SEQ);
        } else {
            self.last_tick = Some(tick);
            self.seq = 0;
        }
        tick * MICROS_PER_SECOND / TICKS_PER_SECOND + self.seq
    }

    fn ensure_names(&mut self, at: (u64, u64), process: Option<&str>, thread: Option<&str>) {
        if let Some(name) = process
            && self.seen_processes.insert(at.0)
        {
            self.put(&json!({"name": "process_name", "ph": "M", "pid": at.0, "tid": 0, "args": {"name": name}}));
        }
        if let Some(name) = thread
            && self.seen_threads.insert(at)
        {
            self.put(&json!({"name": "thread_name", "ph": "M", "pid": at.0, "tid": at.1, "args": {"name": name}}));
        }
    }

    fn event(&mut self, ph: &str, name: &str, at: (u64, u64), tick: u64, args: Map<String, Value>) {
        let mut obj = Map::new();
        obj.insert("name".into(), name.into());
        obj.insert("ph".into(), ph.into());
        obj.insert("pid".into(), at.0.into());
        obj.insert("tid".into(), at.1.into());
        obj.insert("ts".into(), self.next_ts(tick).into());
        if ph == "i" {
            obj.insert("s".into(), "t".into());
        }
        obj.insert("args".into(), Value::Object(args));
        self.put(&Value::Object(obj));
    }
}

/// Reserved fields plus free-form args collected from a span/event.
#[derive(Default)]
struct Fields {
    pid: Option<u64>,
    tid: Option<u64>,
    tick: Option<u64>,
    name: Option<String>,
    process: Option<String>,
    thread: Option<String>,
    kind: Option<String>,
    /// Flow event id (Chrome `args.id`); set by `trace.flow_id`.
    flow_id: Option<u64>,
    args: Map<String, Value>,
}

impl Fields {
    fn text(&mut self, field: &str, value: String) {
        match field {
            "trace.name" => self.name = Some(value),
            "trace.process" => self.process = Some(value),
            "trace.thread" => self.thread = Some(value),
            "trace.kind" => self.kind = Some(value),
            other => {
                self.args.insert(other.into(), Value::String(value));
            }
        }
    }
}

impl Visit for Fields {
    fn record_u64(&mut self, field: &Field, value: u64) {
        match field.name() {
            "trace.pid" => self.pid = Some(value),
            "trace.tid" => self.tid = Some(value),
            "tick" => self.tick = Some(value),
            "trace.flow_id" => self.flow_id = Some(value),
            other => {
                self.args.insert(other.into(), value.into());
            }
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.args.insert(field.name().into(), value.into());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        let number = Number::from_f64(value).map_or(Value::Null, Value::Number);
        self.args.insert(field.name().into(), number);
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.args.insert(field.name().into(), value.into());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.text(field.name(), value.to_string());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.text(field.name(), format!("{value:?}"));
    }
}

/// Identity and args stored on a span so children and `E` events can reuse them.
struct SpanData {
    at: (u64, u64),
    tick: u64,
    name: String,
    late_args: Map<String, Value>,
}

impl<S, W> Layer<S> for ChromeTraceLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: Write + Send + 'static,
{
    fn enabled(&self, metadata: &Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        metadata.target() == TRACE_TARGET
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if attrs.metadata().target() != TRACE_TARGET {
            return;
        }
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        let Some(span) = ctx.span(id) else { return };
        let inherited = span.parent().and_then(|parent| {
            parent
                .extensions()
                .get::<SpanData>()
                .map(|data| (data.at, data.tick))
        });
        let (parent_at, parent_tick) = inherited.unwrap_or(((0, 0), 0));
        let at = (
            fields.pid.unwrap_or(parent_at.0),
            fields.tid.unwrap_or(parent_at.1),
        );
        let tick = fields.tick.unwrap_or(parent_tick);
        let name = fields
            .name
            .take()
            .unwrap_or_else(|| attrs.metadata().name().to_string());
        {
            let mut state = lock(&self.shared);
            state.ensure_names(at, fields.process.as_deref(), fields.thread.as_deref());
            state.event("B", &name, at, tick, fields.args);
        }
        span.extensions_mut().insert(SpanData {
            at,
            tick,
            name,
            late_args: Map::new(),
        });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        if let Some(data) = extensions.get_mut::<SpanData>() {
            let mut fields = Fields::default();
            values.record(&mut fields);
            data.late_args.extend(fields.args);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if event.metadata().target() != TRACE_TARGET {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let scope = ctx.event_span(event).and_then(|span| {
            span.extensions()
                .get::<SpanData>()
                .map(|data| (data.at, data.tick))
        });
        let (scope_at, scope_tick) = scope.unwrap_or(((0, 0), 0));
        let at = (
            fields.pid.unwrap_or(scope_at.0),
            fields.tid.unwrap_or(scope_at.1),
        );
        let tick = fields.tick.unwrap_or(scope_tick);
        let ph = match fields.kind.as_deref() {
            Some("counter") => "C",
            Some("flow_start") => "s",
            Some("flow_end") => "f",
            _ => "i",
        };
        let name = fields.name.take().unwrap_or_default();
        let cat = if ph == "s" || ph == "f" {
            Some("pass")
        } else {
            None
        };
        if let Some(id) = fields.flow_id {
            fields.args.insert("id".into(), id.into());
        }
        let mut state = lock(&self.shared);
        state.ensure_names(at, fields.process.as_deref(), fields.thread.as_deref());
        // Flow events have no `name`/`cat` field at the top level by default,
        // but Chrome's spec uses `name` to group them visually in Perfetto.
        // We adopt `"pass"` as the canonical name so flows render grouped.
        state.event(
            ph,
            cat.unwrap_or(&name),
            at,
            tick,
            fields.args,
        );
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let mut extensions = span.extensions_mut();
        if let Some(data) = extensions.remove::<SpanData>() {
            lock(&self.shared).event("E", &data.name, data.at, data.tick, data.late_args);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::capture_trace as capture;
    use crate::capture_trace_with;
    use crate::emit::{self, Track};
    use crate::metadata::TraceMetadata;
    use serde_json::Value;

    const PLAYER: Track<'static> = Track {
        pid: 1007,
        tid: emit::TID_DECISION,
        process: "Home 7 — CentralMidfielder",
        thread: "Decision",
    };

    fn of<'a>(events: &'a [Value], ph: &str) -> Vec<&'a Value> {
        events.iter().filter(|e| e["ph"] == ph).collect()
    }

    #[test]
    fn empty_run_is_a_valid_empty_array() {
        assert!(capture(|| {}).is_empty());
    }

    #[test]
    fn counter_event_is_typed_and_uses_sim_time() {
        let events = capture(|| emit::counter(PLAYER, 120, "x", 34.5));
        let c = of(&events, "C");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["name"], "x");
        assert_eq!(c[0]["pid"], 1007);
        assert_eq!(c[0]["ts"], 120_u64 * 1_000_000 / 60);
        assert_eq!(c[0]["args"]["value"], 34.5);
        assert!(
            c[0]["args"]["value"].is_f64(),
            "args must be numbers, not strings"
        );
    }

    #[test]
    fn process_and_thread_metadata_are_emitted_once() {
        let events = capture(|| {
            emit::counter(PLAYER, 0, "x", 1.0);
            emit::counter(PLAYER, 60, "x", 2.0);
        });
        let meta = of(&events, "M");
        let procs: Vec<_> = meta
            .iter()
            .filter(|e| e["name"] == "process_name")
            .collect();
        let threads: Vec<_> = meta.iter().filter(|e| e["name"] == "thread_name").collect();
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0]["pid"], 1007);
        assert_eq!(procs[0]["args"]["name"], "Home 7 — CentralMidfielder");
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0]["args"]["name"], "Decision");
    }

    #[test]
    fn spans_nest_and_children_inherit_identity() {
        let events = capture(|| {
            let root = emit::decision_span(PLAYER, 60);
            let shoot = emit::action_span(&root, "ShootAtGoal", true, 0.75);
            emit::consideration(&shoot, "distance_to_goal", 18.4, "logistic", 0.8, 0.29);
            drop(shoot);
            let pass = emit::action_span(&root, "PassTo", false, 0.31);
            drop(pass);
            drop(root);
        });
        let order: Vec<(String, String)> = events
            .iter()
            .filter(|e| e["ph"] != "M")
            .map(|e| {
                (
                    e["ph"].as_str().unwrap_or("?").to_string(),
                    e["name"].as_str().unwrap_or("?").to_string(),
                )
            })
            .collect();
        let expect = |ph: &str, n: &str| (ph.to_string(), n.to_string());
        assert_eq!(
            order,
            vec![
                expect("B", "decision @ tick 60"),
                expect("B", "ShootAtGoal"),
                expect("i", "distance_to_goal"),
                expect("E", "ShootAtGoal"),
                expect("B", "PassTo"),
                expect("E", "PassTo"),
                expect("E", "decision @ tick 60"),
            ]
        );
        for e in events.iter().filter(|e| e["ph"] != "M") {
            assert_eq!(
                (e["pid"].as_u64(), e["tid"].as_u64()),
                (Some(1007), Some(2)),
                "{e}"
            );
        }
        let ts: Vec<u64> = events
            .iter()
            .filter(|e| e["ph"] != "M")
            .map(|e| e["ts"].as_u64().expect("ts"))
            .collect();
        assert!(
            ts.windows(2).all(|w| w[0] < w[1]),
            "ts strictly increasing: {ts:?}"
        );
    }

    #[test]
    fn action_result_and_consideration_args_are_typed() {
        let events = capture(|| {
            let root = emit::decision_span(PLAYER, 0);
            let a = emit::action_span(&root, "ShootAtGoal", true, 0.5);
            emit::consideration(&a, "distance_to_goal", 18.0, "logistic", 0.8, 0.25);
        });
        let b = events
            .iter()
            .find(|e| e["ph"] == "B" && e["name"] == "ShootAtGoal")
            .expect("B");
        assert_eq!(b["args"]["chosen"], true);
        assert_eq!(b["args"]["aggregate_score"], 0.5);
        let i = events.iter().find(|e| e["ph"] == "i").expect("instant");
        assert_eq!(i["s"], "t");
        assert_eq!(i["args"]["curve"], "logistic");
        assert_eq!(i["args"]["raw"], 18.0);
        assert_eq!(
            i["args"]["weight"].as_f64().map(|w| (w * 10.0).round()),
            Some(8.0)
        );
    }

    #[test]
    fn ball_and_referee_instants_carry_args() {
        let ball = Track {
            pid: emit::PID_BALL,
            tid: 1,
            process: "Ball",
            thread: "State",
        };
        let events = capture(|| {
            emit::state_change(ball, 60, "Free", "Possessed");
            emit::possession_change(ball, 60, "none", "12");
        });
        let sc = events
            .iter()
            .find(|e| e["name"] == "state_change")
            .expect("state_change");
        assert_eq!(
            (sc["args"]["from"].as_str(), sc["args"]["to"].as_str()),
            (Some("Free"), Some("Possessed"))
        );
        let pc = events
            .iter()
            .find(|e| e["name"] == "possession_change")
            .expect("possession_change");
        assert_eq!(pc["args"]["to"], "12");
    }

    #[test]
    fn a_ticks_events_never_reach_the_next_ticks_timestamps() {
        // A paused clock repeats one tick, so the per-tick sequence number can
        // grow without bound; it must saturate inside the tick's time slot.
        let events = capture(|| {
            for _ in 0..20_000 {
                emit::counter(PLAYER, 0, "x", 1.0);
            }
            emit::counter(PLAYER, 1, "x", 2.0);
        });
        let stamps: Vec<u64> = events
            .iter()
            .filter(|e| e["ph"] == "C")
            .map(|e| e["ts"].as_u64().expect("ts"))
            .collect();
        let (last, first_of_tick_0) = (stamps[stamps.len() - 1], stamps[0]);
        assert_eq!(first_of_tick_0, 0);
        assert_eq!(last, 1_000_000 / 60, "tick 1 starts at its own base time");
        assert!(
            stamps[..stamps.len() - 1].iter().all(|ts| *ts < last),
            "tick 0 events must stay below tick 1's base timestamp"
        );
        assert!(
            stamps.windows(2).all(|w| w[0] <= w[1]),
            "never goes backwards"
        );
    }

    #[test]
    fn non_finite_floats_serialise_as_null() {
        let events = capture(|| emit::counter(PLAYER, 0, "x", f32::NAN));
        assert!(of(&events, "C")[0]["args"]["value"].is_null());
    }

    #[test]
    fn foreign_targets_are_ignored() {
        let events = capture(|| {
            tracing::info!("hello");
            tracing::info_span!("other").in_scope(|| tracing::warn!("x"));
        });
        assert!(events.is_empty());
    }

    fn metadata_event(events: &[Value]) -> &Value {
        events
            .iter()
            .find(|e| {
                e["ph"] == "M"
                    && e["name"] == "process_labels"
                    && e["pid"] == 0
                    && e["tid"] == 0
            })
            .expect("process_labels metadata event present")
    }

    #[test]
    fn metadata_event_marks_microsecond_time_unit_for_perfetto() {
        // RED: this fails because no metadata event is emitted today, so
        // Perfetto reads our microsecond `ts` values as milliseconds and
        // renders the timeline 1000x too slow.
        let events = capture_trace_with(
            || {
                emit::counter(PLAYER, 0, "x", 1.0);
            },
            Some(TraceMetadata::new(42, 600, None)),
        );
        let m = metadata_event(&events);
        assert_eq!(m["args"]["displayTimeUnit"], "us");
        assert_eq!(m["args"]["seed"], 42);
        assert_eq!(m["args"]["interval_ticks"], 600);
    }

    #[test]
    fn metadata_event_carries_full_range_when_configured() {
        let events = capture_trace_with(
            || {
                emit::counter(PLAYER, 0, "x", 1.0);
            },
            Some(TraceMetadata::new(7, 60, Some((100, 160)))),
        );
        let m = metadata_event(&events);
        assert_eq!(m["args"]["full_range"], serde_json::json!([100, 160]));
    }

    #[test]
    fn no_metadata_event_is_emitted_when_metadata_is_absent() {
        // The default `ChromeTraceLayer::new` path must not write a metadata
        // event: tests and library callers without metadata should get the
        // same minimal output they got before this feature landed.
        let events = capture(|| emit::counter(PLAYER, 0, "x", 1.0));
        assert!(
            events.iter().all(|e| e["name"] != "process_labels"),
            "no process_labels event should be present without metadata"
        );
    }
}
