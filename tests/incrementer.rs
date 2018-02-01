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
enum CounterSynapse {
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

impl Synapse for CounterSynapse {
    type Terminal = CounterTerminal;
    type Dendrite = CounterDendrite;

    fn synapse(self) -> (Self::Terminal, Self::Dendrite) {
        match self {
            CounterSynapse::Increment => {
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
enum IncrementerSynapse {
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

impl Synapse for IncrementerSynapse {
    type Terminal = IncrementerTerminal;
    type Dendrite = IncrementerDendrite;

    fn synapse(self) -> (Self::Terminal, Self::Dendrite) {
        match self {
            IncrementerSynapse::Increment => {
                let (tx, rx) = unsync::mpsc::channel(10);

                (
                    IncrementerTerminal::Incrementer(tx),
                    IncrementerDendrite::Counter(rx),
                )
            },
        }
    }
}

impl From<IncrementerSynapse> for CounterSynapse {
    fn from(synapse: IncrementerSynapse) -> Self {
        match synapse {
            IncrementerSynapse::Increment => CounterSynapse::Increment,
        }
    }
}
impl From<CounterSynapse> for IncrementerSynapse {
    fn from(synapse: CounterSynapse) -> Self {
        match synapse {
            CounterSynapse::Increment => IncrementerSynapse::Increment,
        }
    }
}

impl From<IncrementerTerminal> for CounterTerminal {
    fn from(synapse: IncrementerTerminal) -> Self {
        match synapse {
            IncrementerTerminal::Incrementer(tx) => {
                CounterTerminal::Incrementer(tx)
            },
        }
    }
}
impl From<CounterTerminal> for IncrementerTerminal {
    fn from(synapse: CounterTerminal) -> Self {
        match synapse {
            CounterTerminal::Incrementer(tx) => {
                IncrementerTerminal::Incrementer(tx)
            },
        }
    }
}

impl From<IncrementerDendrite> for CounterDendrite {
    fn from(synapse: IncrementerDendrite) -> Self {
        match synapse {
            IncrementerDendrite::Counter(rx) => CounterDendrite::Counter(rx),
        }
    }
}
impl From<CounterDendrite> for IncrementerDendrite {
    fn from(synapse: CounterDendrite) -> Self {
        match synapse {
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
            vec![Constraint::One(IncrementerSynapse::Increment)],
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
    type Synapse = IncrementerSynapse;
    type Error = Error;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddTerminal(
                _,
                IncrementerSynapse::Increment,
                IncrementerTerminal::Incrementer(tx),
            ) => {
                println!("incrementer got output");

                Ok(Self {
                    timer: self.timer,
                    tx: Some(tx),
                })
            },
            Impulse::Start(_, tx, handle) => {
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
            vec![Constraint::One(CounterSynapse::Increment)],
            vec![],
        )
    }
}

impl Soma for Counter {
    type Synapse = CounterSynapse;
    type Error = Error;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(
                _,
                CounterSynapse::Increment,
                CounterDendrite::Counter(rx),
            ) => {
                println!("counter got input");

                Ok(Self { rx: Some(rx) })
            },
            Impulse::Start(_, tx, handle) => {
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
        .connect(incrementer, counter, IncrementerSynapse::Increment)
        .unwrap();

    core.run(organelle.run(handle)).unwrap();
}
