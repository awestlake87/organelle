use super::{Effector, Error, Handle, Impulse, Result, Soma};

/// data returned by probe operation
#[derive(Debug)]
pub enum ProbeData {
    /// probe returned a soma with the given name
    Soma(String),
    /// probe returned an organelle
    Organelle {
        /// the main soma's name
        soma: String,
        /// all nodes in the organelle
        children: Vec<ProbeData>,
    },
}

/// signal sent to probe soma
#[derive(Debug)]
pub enum ProbeSignal {
    /// request a probe
    RequestProbe,
    /// repond to a probe
    RespondProbe(ProbeData),
}
/// connections that can be made to the probe
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ProbeSynapse {
    /// soma that issues probe requests and receives probe responses
    ProbeController,
}

/// soma that facilitates probes into the structure of organelles
pub struct ProbeSoma {
    effector: Option<Effector<ProbeSignal, ProbeSynapse>>,
    parent: Option<Handle>,
}

impl ProbeSoma {
    /// create a new probe soma
    pub fn new() -> Self {
        Self {
            parent: None,
            effector: None,
        }
    }
}

impl Soma for ProbeSoma {
    type Signal = ProbeSignal;
    type Synapse = ProbeSynapse;
    type Error = Error;

    fn update(self, msg: Impulse<ProbeSignal, ProbeSynapse>) -> Result<Self> {
        match msg {
            Impulse::Init(parent, effector) => Ok(Self {
                parent: parent,
                effector: Some(effector),
            }),
            Impulse::Start => Ok(self),
            Impulse::Signal(_, ProbeSignal::RequestProbe) => {
                if let Some(parent) = self.parent {
                    self.effector.as_ref().unwrap().probe(parent);
                } else {
                    bail!("probe soma cannot be used standalone")
                }
                Ok(self)
            },
            _ => Ok(self),
        }
    }
}
