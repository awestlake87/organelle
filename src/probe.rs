use super::{Axon, Dendrite, Effector, Error, Handle, Impulse, Result, Soma};

/// data returned by probe operation
#[derive(Debug)]
pub enum ProbeData {
    Soma(String),
    Organelle {
        soma: String,
        children: Vec<ProbeData>,
    },
}

/// signal sent to probe soma
#[derive(Debug)]
pub enum ProbeSignal {
    RequestProbe,
    RespondProbe(ProbeData),
}
/// connections that can be made to the probe
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ProbeSynapse {
    ProbeController,
}

pub struct ProbeSoma {
    effector: Option<Effector<ProbeSignal, ProbeSynapse>>,
    parent: Option<Handle>,
}

impl ProbeSoma {
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
