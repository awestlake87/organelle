#[macro_use]
extern crate error_chain;

extern crate organelle;
extern crate tokio_core;

use organelle::{Result, Soma};
use organelle::visualizer;
use tokio_core::reactor;

quick_main!(|| -> Result<()> {
    let mut core = reactor::Core::new()?;

    let handle = core.handle();
    let visualizer = visualizer::Soma::organelle(
        visualizer::Settings::default().open_on_start(true),
        handle.clone(),
    )?;

    core.run(visualizer.run(handle))?;

    Ok(())
});
