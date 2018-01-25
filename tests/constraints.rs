#![feature(proc_macro, conservative_impl_trait, generators)]

#[macro_use]
extern crate error_chain;

extern crate futures_await as futures;
extern crate organelle;
extern crate tokio_core;

use futures::prelude::*;
use futures::unsync;
use organelle::*;
use tokio_core::reactor;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
enum Synapse {
    GiveSomething,
}

#[derive(Debug)]
enum Terminal {
    Giver(unsync::mpsc::Sender<()>),
}

#[derive(Debug)]
enum Dendrite {
    Taker(unsync::mpsc::Receiver<()>),
}

impl organelle::Synapse for Synapse {
    type Terminal = Terminal;
    type Dendrite = Dendrite;

    fn synapse(self) -> (Self::Terminal, Self::Dendrite) {
        match self {
            Synapse::GiveSomething => {
                let (tx, rx) = unsync::mpsc::channel(1);

                (Terminal::Giver(tx), Dendrite::Taker(rx))
            },
        }
    }
}

struct GiverSoma {
    tx: Option<unsync::mpsc::Sender<()>>,
}

impl GiverSoma {
    fn axon() -> Axon<Self> {
        Axon::new(
            GiverSoma { tx: None },
            vec![],
            vec![Constraint::One(Synapse::GiveSomething)],
        )
    }
}

impl Soma for GiverSoma {
    type Synapse = Synapse;
    type Error = Error;

    #[async(boxed)]
    fn update(self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddTerminal(
                Synapse::GiveSomething,
                Terminal::Giver(tx),
            ) => Ok(Self { tx: Some(tx) }),
            Impulse::Start(_, _) => {
                await!(
                    self.tx
                        .unwrap()
                        .send(())
                        .map_err(|_| Error::from("unable to give something"))
                )?;

                Ok(Self { tx: None })
            },
            _ => bail!("unexpected impulse"),
        }
    }
}

struct TakerSoma {
    rx: Option<unsync::mpsc::Receiver<()>>,
}

impl TakerSoma {
    fn axon() -> Axon<Self> {
        Axon::new(
            TakerSoma { rx: None },
            vec![Constraint::One(Synapse::GiveSomething)],
            vec![],
        )
    }
}

impl Soma for TakerSoma {
    type Synapse = Synapse;
    type Error = Error;

    #[async(boxed)]
    fn update(self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(
                Synapse::GiveSomething,
                Dendrite::Taker(rx),
            ) => Ok(Self { rx: Some(rx) }),
            Impulse::Start(tx, _) => {
                await!(
                    self.rx
                        .unwrap()
                        .for_each(move |_| tx.clone()
                            .send(Impulse::Stop)
                            .map(|_| ())
                            .map_err(|_| ()))
                        .map_err(|_| Error::from("unable to stop"))
                )?;

                Ok(Self { rx: None })
            },
            _ => bail!("unexpected impulse"),
        }
    }
}

#[test]
fn test_invalid_input() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();

    let mut organelle = Organelle::new(GiverSoma::axon(), handle.clone());

    let giver1 = organelle.nucleus();
    let giver2 = organelle.add_soma(GiverSoma::axon());

    organelle
        .connect(giver1, giver2, Synapse::GiveSomething)
        .unwrap();

    if let Err(e) = core.run(organelle.run(handle)) {
        match e.kind() {
            &ErrorKind::InvalidSynapse(ref msg) => {
                println!("got expected error: {}", *msg)
            },
            _ => panic!("GiverSoma spewed an unexpected error: {:#?}", e),
        }
    } else {
        panic!("GiverSoma should not accept this input")
    }
}

#[test]
fn test_require_one() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();

    // make sure require works as intended
    {
        let mut organelle = Organelle::new(GiverSoma::axon(), handle.clone());

        let giver = organelle.nucleus();
        let taker = organelle.add_soma(TakerSoma::axon());

        organelle
            .connect(giver, taker, Synapse::GiveSomething)
            .unwrap();

        core.run(organelle.run(handle.clone())).unwrap();
    }

    // make sure require one fails as intended
    {
        if let Err(e) = core.run(TakerSoma::axon().run(handle.clone())) {
            match e.kind() {
                &ErrorKind::MissingSynapse(ref msg) => {
                    println!("got expected error: {}", *msg)
                },
                _ => panic!("unexpected error: {:#?}", e),
            }
        } else {
            panic!("TakerSoma has no input, so it should fail")
        }

        if let Err(e) = core.run(GiverSoma::axon().run(handle.clone())) {
            match e.kind() {
                &ErrorKind::MissingSynapse(ref msg) => {
                    println!("got expected error: {}", *msg)
                },
                _ => panic!("unexpected error: {:#?}", e),
            }
        } else {
            panic!("GiverSoma has no input, so it should fail")
        }
    }
}
