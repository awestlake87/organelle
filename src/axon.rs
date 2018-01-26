use std::collections::HashMap;

use futures::prelude::*;
use futures::unsync::oneshot;

use super::{Error, ErrorKind, Impulse, Result, Soma};
use probe::ProbeData;

/// constraints that can be put on axons for validation purposes
pub enum Constraint<R> {
    /// only accept one synapse
    One(R),
    /// accept any number of synapses
    Variadic(R),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Requirement {
    Unmet,
    Met,
}

/// wrap a soma with a set of requirements that will be validated upon startup
pub struct Axon<T: Soma + 'static> {
    soma: T,
    dendrites: HashMap<T::Synapse, (Constraint<T::Synapse>, Requirement)>,
    terminals: HashMap<T::Synapse, (Constraint<T::Synapse>, Requirement)>,
}

impl<T: Soma + 'static> Axon<T> {
    /// wrap a soma with constraints specified by dendrite and terminal
    /// constraints
    pub fn new(
        soma: T,
        dendrites: Vec<Constraint<T::Synapse>>,
        terminals: Vec<Constraint<T::Synapse>>,
    ) -> Self {
        Self {
            soma: soma,
            dendrites: dendrites
                .iter()
                .map(|d| match d {
                    &Constraint::One(r) => {
                        (r, (Constraint::One(r), Requirement::Unmet))
                    },
                    &Constraint::Variadic(r) => {
                        (r, (Constraint::Variadic(r), Requirement::Met))
                    },
                })
                .collect(),
            terminals: terminals
                .iter()
                .map(|d| match d {
                    &Constraint::One(r) => {
                        (r, (Constraint::One(r), Requirement::Unmet))
                    },
                    &Constraint::Variadic(r) => {
                        (r, (Constraint::Variadic(r), Requirement::Met))
                    },
                })
                .collect(),
        }
    }

    fn add_dendrite(&mut self, synapse: T::Synapse) -> Result<()> {
        if let Some(&mut (ref mut constraint, ref mut req)) =
            self.dendrites.get_mut(&synapse)
        {
            match constraint {
                &mut Constraint::One(_) => match req {
                    &mut Requirement::Unmet => *req = Requirement::Met,
                    &mut Requirement::Met => bail!(ErrorKind::InvalidSynapse(
                        format!("expected only one dendrite for {:?}", synapse)
                    )),
                },
                &mut Constraint::Variadic(_) => (),
            }
        } else {
            bail!(ErrorKind::InvalidSynapse(format!(
                "no constraints found for {:?}",
                synapse
            )))
        }

        Ok(())
    }

    fn add_terminal(&mut self, synapse: T::Synapse) -> Result<()> {
        if let Some(&mut (ref mut constraint, ref mut req)) =
            self.terminals.get_mut(&synapse)
        {
            match constraint {
                &mut Constraint::One(_) => match req {
                    &mut Requirement::Unmet => *req = Requirement::Met,
                    &mut Requirement::Met => bail!(ErrorKind::InvalidSynapse(
                        format!("expected only one terminal for {:?}", synapse)
                    )),
                },
                &mut Constraint::Variadic(_) => (),
            }
        } else {
            bail!(ErrorKind::InvalidSynapse(format!(
                "no constraints found for {:?}",
                synapse
            )))
        }

        Ok(())
    }

    fn start(&self) -> Result<()> {
        for (synapse, &(ref constraint, ref req)) in &self.dendrites {
            match constraint {
                &Constraint::One(_) => match req {
                    &Requirement::Met => (),
                    &Requirement::Unmet => bail!(ErrorKind::MissingSynapse(
                        format!("expected dendrite synapse for {:?}", *synapse)
                    )),
                },
                &Constraint::Variadic(_) => assert_eq!(*req, Requirement::Met),
            }
        }

        for (synapse, &(ref constraint, ref req)) in &self.terminals {
            match constraint {
                &Constraint::One(_) => match req {
                    &Requirement::Met => (),
                    &Requirement::Unmet => bail!(ErrorKind::MissingSynapse(
                        format!("expected terminal synapse for {:?}", *synapse)
                    )),
                },
                &Constraint::Variadic(_) => assert_eq!(*req, Requirement::Met),
            }
        }

        Ok(())
    }

    #[async]
    fn probe(self, tx: oneshot::Sender<ProbeData>) -> Result<Self> {
        let (axon, data) = await!(self.probe_data())?;

        if let Err(_) = tx.send(data) {
            // rx does not care anymore
        }

        Ok(axon)
    }
}

impl<T: Soma + 'static> Soma for Axon<T> {
    type Synapse = T::Synapse;
    type Error = Error;

    #[async(boxed)]
    fn probe_data(self) -> Result<(Self, ProbeData)> {
        Ok((self, ProbeData::Axon))
    }

    #[async(boxed)]
    fn update(mut self, imp: Impulse<T::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(synapse, _) => {
                self.add_dendrite(synapse)?;

                self.soma =
                    await!(self.soma.update(imp)).map_err(|e| e.into())?;

                Ok(self)
            },
            Impulse::AddTerminal(synapse, _) => {
                self.add_terminal(synapse)?;

                self.soma =
                    await!(self.soma.update(imp)).map_err(|e| e.into())?;

                Ok(self)
            },
            Impulse::Start(_, _) => {
                self.start()?;

                self.soma =
                    await!(self.soma.update(imp)).map_err(|e| e.into())?;

                Ok(self)
            },

            Impulse::Probe(tx) => await!(self.probe(tx)),

            Impulse::Stop | Impulse::Error(_) => {
                bail!("unexpected impulse in axon")
            },
            //_ => await!(self.soma.update(imp))?,
        }
    }
}
