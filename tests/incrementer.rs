#![feature(proc_macro, conservative_impl_trait, generators)]

#[macro_use]
extern crate error_chain;

extern crate futures_await as futures;
extern crate organelle;
extern crate tokio_core;
extern crate tokio_timer;
extern crate uuid;

use std::mem;
use std::time;

use futures::prelude::*;
use futures::unsync;
use organelle::*;
use tokio_core::reactor;
use tokio_timer::Timer;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
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

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
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

struct Incrementer {
    timer: Option<Timer>,
    tx: Option<unsync::mpsc::Sender<()>>,
}

impl Incrementer {
    fn axon() -> Axon<Self> {
        Axon::new(
            Self {
                timer: Some(Timer::default()),
                tx: None,
            },
            vec![],
            vec![Dendrite::One(IncrementerRole::Counter)],
        )
    }

    #[async]
    fn increment(sender: unsync::mpsc::Sender<()>, timer: Timer) -> Result<()> {
        loop {
            await!(
                timer
                    .sleep(time::Duration::from_millis(250))
                    .map_err(|e| Error::with_chain(e, ErrorKind::SomaError))
            )?;

            await!(
                sender
                    .clone()
                    .send(())
                    .map_err(|_| Error::from("unable to increment"))
            )?;
        }
    }
}

impl Soma for Incrementer {
    type Role = IncrementerRole;
    type Synapse = IncrementerSynapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(
        mut self,
        imp: Impulse<Self::Role, Self::Synapse>,
    ) -> Result<Self> {
        match imp {
            Impulse::AddOutput(
                IncrementerRole::Counter,
                IncrementerSynapse::Output(tx),
            ) => {
                println!("incrementer got output");

                Ok(Self {
                    timer: self.timer,
                    tx: Some(tx),
                })
            },
            Impulse::Start(tx, handle) => {
                let sender = self.tx.as_ref().unwrap().clone();
                let timer = mem::replace(&mut self.timer, None).unwrap();

                handle.spawn(Self::increment(sender, timer).or_else(|e| {
                    tx.send(Impulse::Error(e)).map(|_| ()).map_err(|_| ())
                }));

                Ok(self)
            },

            _ => bail!("unexpected impulse"),
        }
    }
}

struct Counter {
    rx: Option<unsync::mpsc::Receiver<()>>,
}

impl Counter {
    fn axon() -> Axon<Self> {
        Axon::new(
            Self { rx: None },
            vec![Dendrite::One(CounterRole::Incrementer)],
            vec![],
        )
    }
}

impl Soma for Counter {
    type Role = CounterRole;
    type Synapse = CounterSynapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(
        mut self,
        imp: Impulse<Self::Role, Self::Synapse>,
    ) -> Result<Self> {
        match imp {
            Impulse::AddInput(
                CounterRole::Incrementer,
                CounterSynapse::Input(rx),
            ) => {
                println!("counter got input");

                Ok(Self { rx: Some(rx) })
            },
            Impulse::Start(tx, handle) => {
                let stopper = tx.clone();
                let rx = mem::replace(&mut self.rx, None).unwrap();

                handle.spawn(
                    async_block! {
                        let mut i = 0;

                        await!(
                            rx.take_while(move |_| {
                                i += 1;

                                println!("counter {}...", i);

                                Ok((i < 5))
                            }).for_each(|_| Ok(()))
                                .and_then(|_| Ok(()))
                                .or_else(|_| Err(Error::from(
                                    "unable to receive increment"
                                )))
                        )?;

                        await!(stopper.send(Impulse::Stop)
                            .map_err(|_| Error::from(
                                "unable to stop gracefully"
                            ))
                        )?;

                        Ok(())
                    }.or_else(|e| {
                        tx.send(Impulse::Error(e)).map(|_| ()).map_err(|_| ())
                    }),
                );

                Ok(self)
            },

            _ => bail!("unexpected impulse"),
        }
    }
}

#[test]
fn test_organelle() {
    let mut core = reactor::Core::new().unwrap();
    let handle = core.handle();

    let mut organelle = Organelle::new(Incrementer::axon(), handle.clone());

    let incrementer = organelle.main();
    let counter = organelle.add_soma(Counter::axon());

    organelle
        .connect(incrementer, counter, IncrementerRole::Counter)
        .unwrap();

    core.run(organelle.run(handle)).unwrap();
}
