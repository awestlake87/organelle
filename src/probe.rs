use futures::prelude::*;
use futures::unsync::{mpsc, oneshot};
use tokio_core::reactor;
use uuid::Uuid;

use super::{Error, Result};
use axon::{Axon, Constraint};
use soma::{self, Impulse};

/// data associated with a synapse between two somas
#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct SynapseData(pub String);

/// data associated with a synapse constraint
#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum ConstraintData {
    /// only one synapse of the given variant
    #[serde(rename = "one")]
    One {
        /// the enum variant for the synapse
        variant: String,
        /// the other soma involved in the synapse
        soma: Uuid,
    },

    /// any number of synapses of the given variant
    #[serde(rename = "variadic")]
    Variadic {
        /// the enum variant for the synapse
        variant: String,
        /// the other somas involved in the synapses
        somas: Vec<Uuid>,
    },
}

/// data associated with a soma, organelle, or axon
#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
#[serde(tag = "type")]
pub enum SomaData {
    /// data associated with an organelle
    #[serde(rename = "organelle")]
    Organelle {
        /// the soma at the center of an organelle
        nucleus: Box<SomaData>,
        /// the rest of the somas contained in the organelle
        somas: Vec<SomaData>,
        /// unique id of the organelle
        uuid: Uuid,
        /// name of the organelle
        name: String,
    },

    /// data associated with the axon of a soma
    #[serde(rename = "axon")]
    Axon {
        /// data associated with the terminals for this soma
        terminals: Vec<ConstraintData>,
        /// data associated with the dendrites for this soma
        dendrites: Vec<ConstraintData>,
        /// unique id of the axon
        uuid: Uuid,
        /// name of the axon
        name: String,
    },

    /// data associated with a custom soma
    #[serde(rename = "soma")]
    Soma {
        /// the type of synapse used by this soma
        synapse: SynapseData,
        /// the name of the soma
        name: String,
    },
}

/// soma that probes the internal structure of an organelle
pub struct Soma {
    dendrites: Vec<Dendrite>,
}

impl Soma {
    /// create a new probe soma
    pub fn axon() -> Axon<Self> {
        Axon::new(
            Self { dendrites: vec![] },
            vec![Constraint::Variadic(Synapse::Probe)],
            vec![],
        )
    }
}

/// the synapse for a probe
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Synapse {
    /// a synapse used to perform probes
    Probe,
}

/// settings for a probe operation
#[derive(Debug, Clone)]
pub struct Settings;

impl Settings {
    /// create settings
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug)]
enum Request {
    Probe(Settings, oneshot::Sender<SomaData>),
}

/// sender for a probe operation
#[derive(Debug, Clone)]
pub struct Terminal {
    tx: mpsc::Sender<Request>,
}

impl Terminal {
    /// perform the probe
    #[async]
    pub fn probe(self, settings: Settings) -> Result<SomaData> {
        let (tx, rx) = oneshot::channel();

        await!(
            self.tx
                .send(Request::Probe(settings, tx))
                .map(|_| ())
                .map_err(|_| Error::from("unable to send probe request"))
        )?;

        await!(rx.map_err(|_| Error::from("unable to receive probe response")))
    }
}

/// receive for a probe operation
#[derive(Debug)]
pub struct Dendrite {
    rx: mpsc::Receiver<Request>,
}

/// create a junction between two probe-ready somas
pub fn synapse() -> (Terminal, Dendrite) {
    let (tx, rx) = mpsc::channel(10);

    (Terminal { tx: tx }, Dendrite { rx: rx })
}

impl soma::Synapse for Synapse {
    type Terminal = Terminal;
    type Dendrite = Dendrite;

    fn synapse(self) -> (Terminal, Dendrite) {
        match self {
            Synapse::Probe => synapse(),
        }
    }
}

impl soma::Soma for Soma {
    type Synapse = Synapse;
    type Error = Error;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<Self::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(_, Synapse::Probe, rx) => {
                self.dendrites.push(rx);

                Ok(self)
            },

            Impulse::Start(_, main_tx, handle) => {
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
                Request::Probe(settings, tx) => {
                    await!(
                        main_tx
                            .clone()
                            .send(Impulse::Probe(settings, tx))
                            .map_err(|_| "unable to send probe impulse")
                    )?;
                },
            }
        }

        Ok(())
    }
}
