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

#[derive(Debug, Copy, Clone)]
enum Role {
    GiveSomething,
}

enum Synapse {
    Giver(unsync::mpsc::Sender<()>),
    Taker(unsync::mpsc::Receiver<()>),
}

impl From<Role> for (Synapse, Synapse) {
    fn from(role: Role) -> (Synapse, Synapse) {
        match role {
            Role::GiveSomething => {
                let (tx, rx) = unsync::mpsc::channel(1);

                (Synapse::Giver(tx), Synapse::Taker(rx))
            },
        }
    }
}

struct GiverSoma;

impl GiverSoma {
    fn axon() -> Axon<Self> {
        Axon::new(
            GiverSoma {},
            vec![],
            vec![Dendrite::One(Role::GiveSomething)],
        )
    }
}

impl Soma for GiverSoma {
    type Role = Role;
    type Synapse = Synapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<Self::Role, Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddOutput(Role::GiveSomething, Synapse::Giver(tx)) => {
                Ok(self)
            },
            Impulse::Start(_, _) => Ok(self),
            _ => bail!("unexpected impulse"),
        }
    }
}

struct TakerSoma;

impl TakerSoma {
    fn axon() -> Axon<Self> {
        Axon::new(
            TakerSoma {},
            vec![Dendrite::One(Role::GiveSomething)],
            vec![],
        )
    }
}

impl Soma for TakerSoma {
    type Role = Role;
    type Synapse = Synapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<Self::Role, Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddInput(Role::GiveSomething, Synapse::Taker(rx)) => {
                Ok(self)
            },
            Impulse::Start(_, _) => Ok(self),
            _ => bail!("unexpected impulse"),
        }
    }
}

#[test]
fn test_invalid_input() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();

    let mut organelle = Organelle::new(GiverSoma::axon(), handle.clone());

    let giver1 = organelle.main();
    let giver2 = organelle.add_soma(GiverSoma::axon());

    organelle.connect(giver1, giver2, Role::GiveSomething);

    if let Err(e) = core.run(organelle.run(handle)) {
        match e.kind() {
            &ErrorKind::InvalidSynapse => (),
            _ => panic!("GiverSoma spewed an unexpected error: {:#?}", e),
        }
    } else {
        panic!("GiverSoma should not accept this input")
    }
}
