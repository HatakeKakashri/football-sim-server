//! The wire shape of an animation frame (server-API ticket 05).
//!
//! A captured frame names its players by their roster index, so it
//! holds no player id string. The body writer resolves the index back
//! to the fixture id — borrowed from the manifest's roster, never
//! copied — so the bytes on the wire are exactly the bytes the earlier
//! format produced.
//!
//! The resolution is deliberately short-lived: the next ticket writes
//! the index into the body itself and drops the resolution.

use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::api::model::{FrameBall, FramePlayer, MatchFrame, RosterEntry};

/// The frames of a segment body, resolved against the roster.
///
/// The frames are written one by one, straight from the captured
/// frames: the body allocates no copy of them.
#[derive(Debug)]
pub(super) struct FramesBody<'a> {
    /// The frames the segment covers.
    frames: &'a [&'a MatchFrame],
    /// The manifest's roster rows: the index → id mapping.
    roster: &'a [RosterEntry],
}

impl<'a> FramesBody<'a> {
    /// The frames of a segment body, resolved against the manifest's
    /// roster.
    pub(super) const fn new(frames: &'a [&'a MatchFrame], roster: &'a [RosterEntry]) -> Self {
        Self { frames, roster }
    }
}

impl Serialize for FramesBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut frames = serializer.serialize_seq(Some(self.frames.len()))?;
        for frame in self.frames {
            frames.serialize_element(&FrameBody::new(frame, self.roster))?;
        }
        frames.end()
    }
}

/// One frame, resolved against the manifest's roster.
#[derive(Debug)]
struct FrameBody<'a> {
    /// The captured frame.
    frame: &'a MatchFrame,
    /// The manifest's roster rows: the index → id mapping.
    roster: &'a [RosterEntry],
}

impl<'a> FrameBody<'a> {
    /// One frame, resolved against the manifest's roster.
    const fn new(frame: &'a MatchFrame, roster: &'a [RosterEntry]) -> Self {
        Self { frame, roster }
    }
}

impl Serialize for FrameBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut frame = serializer.serialize_struct("MatchFrame", 4)?;
        frame.serialize_field("tick", &self.frame.tick)?;
        frame.serialize_field("half", &self.frame.half)?;
        frame.serialize_field(
            "ball",
            &BallBody {
                ball: &self.frame.ball,
                roster: self.roster,
            },
        )?;
        frame.serialize_field(
            "players",
            &PlayersBody {
                players: &self.frame.players,
                roster: self.roster,
            },
        )?;
        frame.end()
    }
}

/// The ball of a frame, resolved against the manifest's roster.
#[derive(Debug)]
struct BallBody<'a> {
    /// The captured ball.
    ball: &'a FrameBall,
    /// The manifest's roster rows: the index → id mapping.
    roster: &'a [RosterEntry],
}

impl Serialize for BallBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut ball = serializer.serialize_struct("FrameBall", 4)?;
        ball.serialize_field("x", &self.ball.x)?;
        ball.serialize_field("y", &self.ball.y)?;
        ball.serialize_field("state", self.ball.state.as_str())?;
        ball.serialize_field(
            "possessor",
            &self
                .ball
                .possessor
                .map(|row| row.row(self.roster).id.as_str()),
        )?;
        ball.end()
    }
}

/// The players of a frame, resolved against the manifest's roster.
#[derive(Debug)]
struct PlayersBody<'a> {
    /// The captured players.
    players: &'a [FramePlayer],
    /// The manifest's roster rows: the index → id mapping.
    roster: &'a [RosterEntry],
}

impl Serialize for PlayersBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut players = serializer.serialize_seq(Some(self.players.len()))?;
        for player in self.players {
            players.serialize_element(&PlayerBody {
                player,
                roster: self.roster,
            })?;
        }
        players.end()
    }
}

/// One player of a frame, resolved against the manifest's roster.
#[derive(Debug)]
struct PlayerBody<'a> {
    /// The captured player.
    player: &'a FramePlayer,
    /// The manifest's roster rows: the index → id mapping.
    roster: &'a [RosterEntry],
}

impl Serialize for PlayerBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut player = serializer.serialize_struct("FramePlayer", 3)?;
        player.serialize_field(
            "id",
            self.player.roster_index.row(self.roster).id.as_str(),
        )?;
        player.serialize_field("x", &self.player.x)?;
        player.serialize_field("y", &self.player.y)?;
        player.end()
    }
}
