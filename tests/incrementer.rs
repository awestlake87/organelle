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
    Increment,
}

#[derive(Debug)]
enum CounterTerminal {
    Incrementer(unsync::mpsc::Sender<()>),
}

#[derive(Debug)]
enum CounterDendrite {
    Counter(unsync::mpsc::Receiver<()>),
}

impl Role for CounterRole {
    type Terminal = CounterTerminal;
    type Dendrite = CounterDendrite;

    fn synapse(self) -> (Self::Terminal, Self::Dendrite) {
        match self {
            CounterRole::Increment => {
                let (tx, rx) = unsync::mpsc::channel(10);

                (
                    CounterTerminal::Incrementer(tx),
                    CounterDendrite::Counter(rx),
                )
            },
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
enum IncrementerRole {
    Increment,
}

#[derive(Debug)]
enum IncrementerTerminal {
    Incrementer(unsync::mpsc::Sender<()>),
}

#[derive(Debug)]
enum IncrementerDendrite {
    Counter(unsync::mpsc::Receiver<()>),
}

impl Role for IncrementerRole {
    type Terminal = IncrementerTerminal;
    type Dendrite = IncrementerDendrite;

    fn synapse(self) -> (Self::Terminal, Self::Dendrite) {
        match self {
            IncrementerRole::Increment => {
                let (tx, rx) = unsync::mpsc::channel(10);

                (
                    IncrementerTerminal::Incrementer(tx),
                    IncrementerDendrite::Counter(rx),
                )
            },
        }
    }
}

impl From<IncrementerRole> for CounterRole {
    fn from(role: IncrementerRole) -> Self {
        match role {
            IncrementerRole::Increment => CounterRole::Increment,
        }
    }
}
impl From<CounterRole> for IncrementerRole {
    fn from(role: CounterRole) -> Self {
        match role {
            CounterRole::Increment => IncrementerRole::Increment,
        }
    }
}

impl From<IncrementerTerminal> for CounterTerminal {
    fn from(role: IncrementerTerminal) -> Self {
        match role {
            IncrementerTerminal::Incrementer(tx) => {
                CounterTerminal::Incrementer(tx)
            },
        }
    }
}
impl From<CounterTerminal> for IncrementerTerminal {
    fn from(role: CounterTerminal) -> Self {
        match role {
            CounterTerminal::Incrementer(tx) => {
                IncrementerTerminal::Incrementer(tx)
            },
        }
    }
}

impl From<IncrementerDendrite> for CounterDendrite {
    fn from(role: IncrementerDendrite) -> Self {
        match role {
            IncrementerDendrite::Counter(rx) => CounterDendrite::Counter(rx),
        }
    }
}
impl From<CounterDendrite> for IncrementerDendrite {
    fn from(role: CounterDendrite) -> Self {
        match role {
            CounterDendrite::Counter(rx) => IncrementerDendrite::Counter(rx),
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
            vec![Constraint::One(IncrementerRole::Increment)],
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
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Role>) -> Result<Self> {
        match imp {
            Impulse::AddTerminal(
                IncrementerRole::Increment,
                IncrementerTerminal::Incrementer(tx),
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
            vec![Constraint::One(CounterRole::Increment)],
            vec![],
        )
    }
}

impl Soma for Counter {
    type Role = CounterRole;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Role>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(
                CounterRole::Increment,
                CounterDendrite::Counter(rx),
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

    let incrementer = organelle.nucleus();
    let counter = organelle.add_soma(Counter::axon());

    organelle
        .connect(incrementer, counter, IncrementerRole::Increment)
        .unwrap();

    core.run(organelle.run(handle)).unwrap();
}
