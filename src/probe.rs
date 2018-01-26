use futures::prelude::*;
use futures::unsync::{mpsc, oneshot};
use tokio_core::reactor;

use super::{Error, Result};
use axon::{Axon, Constraint};
use soma::{self, Impulse};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProbeData {
    #[serde(rename = "organelle")]
    Organelle {
        nucleus: Box<ProbeData>,
        somas: Vec<ProbeData>,
    },

    #[serde(rename = "axon")] Axon,

    #[serde(rename = "soma")] Soma,
}

pub struct Soma {
    dendrites: Vec<Dendrite>,
}

impl Soma {
    pub fn axon() -> Axon<Self> {
        Axon::new(
            Self { dendrites: vec![] },
            vec![Constraint::Variadic(Synapse::Probe)],
            vec![],
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Synapse {
    Probe,
}

#[derive(Debug)]
enum Request {
    Probe(oneshot::Sender<ProbeData>),
}

#[derive(Debug, Clone)]
pub struct Terminal {
    tx: mpsc::Sender<Request>,
}

impl Terminal {
    #[async]
    pub fn probe(self) -> Result<ProbeData> {
        let (tx, rx) = oneshot::channel();

        await!(
            self.tx
                .send(Request::Probe(tx))
                .map(|_| ())
                .map_err(|_| Error::from("unable to send probe request"))
        )?;

        await!(rx.map_err(|_| Error::from("unable to receive probe response")))
    }
}

#[derive(Debug)]
pub struct Dendrite {
    rx: mpsc::Receiver<Request>,
}

impl soma::Synapse for Synapse {
    type Terminal = Terminal;
    type Dendrite = Dendrite;

    fn synapse(self) -> (Terminal, Dendrite) {
        match self {
            Synapse::Probe => {
                let (tx, rx) = mpsc::channel(10);

                (Terminal { tx: tx }, Dendrite { rx: rx })
            },
        }
    }
}

impl soma::Soma for Soma {
    type Synapse = Synapse;
    type Error = Error;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(Synapse::Probe, rx) => {
                self.dendrites.push(rx);

                Ok(self)
            },

            Impulse::Start(main_tx, handle) => {
                handle.spawn(
                    ProbeTask::run(
                        main_tx.clone(),
                        handle.clone(),
                        self.dendrites,
                    ).or_else(move |e| {
                        main_tx
                            .send(Impulse::Error(e))
                            .map(|_| ())
                            .map_err(|_| ())
                    }),
                );

                Ok(Self { dendrites: vec![] })
            },

            _ => bail!("unexpected impulse"),
        }
    }
}

struct ProbeTask;

impl ProbeTask {
    #[async]
    fn run(
        main_tx: mpsc::Sender<Impulse<Synapse>>,
        handle: reactor::Handle,
        dendrites: Vec<Dendrite>,
    ) -> Result<()> {
        let (tx, rx) = mpsc::channel(10);

        for dendrite in dendrites {
            handle.spawn(
                tx.clone()
                    .send_all(dendrite.rx.map_err(|_| unreachable!()))
                    .map(|_| ())
                    .map_err(|_| ()),
            );
        }

        #[async]
        for req in rx.map_err(|_| -> Error { unreachable!() }) {
            match req {
                Request::Probe(tx) => {
                    await!(
                        main_tx
                            .clone()
                            .send(Impulse::Probe(tx))
                            .map_err(|_| "unable to send probe impulse")
                    )?;
                },
            }
        }

        Ok(())
    }
}
