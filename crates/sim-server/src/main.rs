use sim_server::ServerSimulation;

fn main() {
    println!("Football Simulation Server");
    
    let mut server = ServerSimulation::new(12345);
    
    // Run a simple simulation
    for _ in 0..100 {
        server.simulation.tick(1.0 / 60.0);
    }
    
    println!("Simulation completed with {} ticks", server.simulation.tick);
}