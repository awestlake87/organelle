#![feature(proc_macro, conservative_impl_trait, generators)]

#[macro_use]
extern crate error_chain;

extern crate futures_await as futures;
extern crate organelle;
extern crate tokio_core;
extern crate uuid;

use futures::prelude::*;
use futures::unsync;
use organelle::*;
use tokio_core::reactor;

#[derive(Debug, Copy, Clone)]
enum CounterRole {
    Incrementer,
}

enum CounterSynapse {
    Feed(unsync::mpsc::Sender<()>),
    Input(unsync::mpsc::Receiver<()>),
}

impl From<CounterRole> for (CounterSynapse, CounterSynapse) {
    fn from(role: CounterRole) -> Self {
        match role {
            CounterRole::Incrementer => {
                let (tx, rx) = unsync::mpsc::channel(10);

                (CounterSynapse::Feed(tx), CounterSynapse::Input(rx))
            },
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum IncrementerRole {
    Counter,
}

enum IncrementerSynapse {
    Output(unsync::mpsc::Sender<()>),
    Feed(unsync::mpsc::Receiver<()>),
}

impl From<IncrementerRole> for (IncrementerSynapse, IncrementerSynapse) {
    fn from(role: IncrementerRole) -> Self {
        match role {
            IncrementerRole::Counter => {
                let (tx, rx) = unsync::mpsc::channel(10);

                (IncrementerSynapse::Output(tx), IncrementerSynapse::Feed(rx))
            },
        }
    }
}

impl From<IncrementerRole> for CounterRole {
    fn from(role: IncrementerRole) -> Self {
        match role {
            IncrementerRole::Counter => CounterRole::Incrementer,
        }
    }
}

impl From<CounterRole> for IncrementerRole {
    fn from(role: CounterRole) -> Self {
        match role {
            CounterRole::Incrementer => IncrementerRole::Counter,
        }
    }
}

impl From<IncrementerSynapse> for CounterSynapse {
    fn from(role: IncrementerSynapse) -> Self {
        match role {
            IncrementerSynapse::Output(tx) => CounterSynapse::Feed(tx),
            IncrementerSynapse::Feed(rx) => CounterSynapse::Input(rx),
        }
    }
}

impl From<CounterSynapse> for IncrementerSynapse {
    fn from(role: CounterSynapse) -> Self {
        match role {
            CounterSynapse::Feed(tx) => IncrementerSynapse::Output(tx),
            CounterSynapse::Input(rx) => IncrementerSynapse::Feed(rx),
        }
    }
}

struct Incrementer;

impl Incrementer {
    fn axon() -> Axon<Self> {
        Axon::new(
            Self {},
            vec![],
            vec![Dendrite::One(IncrementerRole::Counter)],
        )
    }
}

impl Soma for Incrementer {
    type Role = IncrementerRole;
    type Synapse = IncrementerSynapse;
    type Future = Box<Future<Item = Self, Error = Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<Self::Role, Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddOutput(
                IncrementerRole::Counter,
                IncrementerSynapse::Output(_),
            ) => {
                println!("incrementer got output");

                Ok(self)
            },
            Impulse::Start(_) => Ok(self),

            _ => bail!("unexpected impulse"),
        }
    }
}

struct Counter;

impl Counter {
    fn axon() -> Axon<Self> {
        Axon::new(
            Self {},
            vec![Dendrite::One(CounterRole::Incrementer)],
            vec![],
        )
    }
}

impl Soma for Counter {
    type Role = CounterRole;
    type Synapse = CounterSynapse;
    type Future = Box<Future<Item = Self, Error = Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<Self::Role, Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddInput(
                CounterRole::Incrementer,
                CounterSynapse::Input(_),
            ) => {
                println!("counter got input");

                Ok(self)
            },
            Impulse::Start(tx) => {
                await!(
                    tx.send(Impulse::Stop)
                        .map_err(|_| Error::from("unable to stop gracefully"))
                )?;
                Ok(self)
            },

            _ => bail!("unexpected impulse"),
        }
    }
}

#[test]
fn test_organelle() {
    let mut core = reactor::Core::new().unwrap();

    let mut organelle =
        Organelle::<IncrementerRole, IncrementerSynapse>::new(core.handle());

    let incrementer = organelle.add_soma(Incrementer::axon());
    let counter = organelle.add_soma(Counter::axon());

    organelle
        .connect(incrementer, counter, IncrementerRole::Counter)
        .unwrap();

    core.run(organelle.run()).unwrap();
}

#[test]
fn test_soma() {
    let mut core = reactor::Core::new().unwrap();

    let counter = Counter {};

    core.run(counter.run()).unwrap();
}
