use std::collections::HashMap;
use std::intrinsics;

use futures::prelude::*;
use futures::unsync::oneshot;
use uuid::Uuid;

use super::{Error, ErrorKind, Result};
use probe::{self, ConstraintData, SomaData};
use soma::{Impulse, Soma, Synapse};

/// constraints that can be put on axons for validation purposes
pub enum Constraint<S: Synapse> {
    /// only accept one synapse
    One(S),
    /// accept any number of synapses
    Variadic(S),
}

#[derive(Debug)]
enum Requirement {
    Unmet,
    MetOne(Uuid),
    MetVariadic(Vec<Uuid>),
}

/// wrap a soma with a set of requirements that will be validated upon startup
pub struct Axon<T: Soma + 'static> {
    soma: T,

    uuid: Option<Uuid>,

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

            uuid: None,

            dendrites: dendrites
                .iter()
                .map(|d| match d {
                    &Constraint::One(r) => {
                        (r, (Constraint::One(r), Requirement::Unmet))
                    },
                    &Constraint::Variadic(r) => (
                        r,
                        (
                            Constraint::Variadic(r),
                            Requirement::MetVariadic(vec![]),
                        ),
                    ),
                })
                .collect(),
            terminals: terminals
                .iter()
                .map(|d| match d {
                    &Constraint::One(r) => {
                        (r, (Constraint::One(r), Requirement::Unmet))
                    },
                    &Constraint::Variadic(r) => (
                        r,
                        (
                            Constraint::Variadic(r),
                            Requirement::MetVariadic(vec![]),
                        ),
                    ),
                })
                .collect(),
        }
    }

    fn add_dendrite(&mut self, uuid: Uuid, synapse: T::Synapse) -> Result<()> {
        if let Some(&mut (ref mut constraint, ref mut req)) =
            self.dendrites.get_mut(&synapse)
        {
            match constraint {
                &mut Constraint::One(_) => match req {
                    &mut Requirement::Unmet => *req = Requirement::MetOne(uuid),
                    &mut Requirement::MetOne(_) => {
                        bail!(ErrorKind::InvalidSynapse(format!(
                            "expected only one dendrite for {:?}",
                            synapse
                        )))
                    },
                    _ => unreachable!(),
                },
                &mut Constraint::Variadic(_) => match req {
                    &mut Requirement::MetVariadic(ref mut dendrites) => {
                        dendrites.push(uuid);
                    },
                    _ => unreachable!(),
                },
            }
        } else {
            bail!(ErrorKind::InvalidSynapse(format!(
                "no constraints found for {:?}",
                synapse
            )))
        }

        Ok(())
    }

    fn add_terminal(&mut self, uuid: Uuid, synapse: T::Synapse) -> Result<()> {
        if let Some(&mut (ref mut constraint, ref mut req)) =
            self.terminals.get_mut(&synapse)
        {
            match constraint {
                &mut Constraint::One(_) => match req {
                    &mut Requirement::Unmet => *req = Requirement::MetOne(uuid),
                    &mut Requirement::MetOne(_) => {
                        bail!(ErrorKind::InvalidSynapse(format!(
                            "expected only one terminal for {:?}",
                            synapse
                        )))
                    },
                    _ => unreachable!(),
                },
                &mut Constraint::Variadic(_) => match req {
                    &mut Requirement::MetVariadic(ref mut terminals) => {
                        terminals.push(uuid);
                    },
                    _ => unreachable!(),
                },
            }
        } else {
            bail!(ErrorKind::InvalidSynapse(format!(
                "no constraints found for {:?}",
                synapse
            )))
        }

        Ok(())
    }

    fn start(&mut self, uuid: Uuid) -> Result<()> {
        self.uuid = Some(uuid);

        for (synapse, &(ref constraint, ref req)) in &self.dendrites {
            match constraint {
                &Constraint::One(_) => match req {
                    &Requirement::MetOne(_) => (),
                    &Requirement::Unmet => bail!(ErrorKind::MissingSynapse(
                        format!("expected dendrite synapse for {:?}", *synapse)
                    )),
                    _ => unreachable!(),
                },
                &Constraint::Variadic(_) => match req {
                    &Requirement::MetVariadic(_) => (),
                    _ => unreachable!(),
                },
            }
        }

        for (synapse, &(ref constraint, ref req)) in &self.terminals {
            match constraint {
                &Constraint::One(_) => match req {
                    &Requirement::MetOne(_) => (),
                    &Requirement::Unmet => bail!(ErrorKind::MissingSynapse(
                        format!("expected terminal synapse for {:?}", *synapse)
                    )),
                    _ => unreachable!(),
                },
                &Constraint::Variadic(_) => match req {
                    &Requirement::MetVariadic(_) => (),
                    _ => unreachable!(),
                },
            }
        }

        Ok(())
    }

    #[async]
    fn perform_probe(
        self,
        settings: probe::Settings,
        tx: oneshot::Sender<SomaData>,
    ) -> Result<Self> {
        let (axon, data) = await!(self.probe(settings))?;

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
    fn probe(self, _settings: probe::Settings) -> Result<(Self, SomaData)> {
        let terminals = self.terminals
            .iter()
            .map(|(synapse, &(ref constraint, ref requirement))| {
                match constraint {
                    &Constraint::One(_) => ConstraintData::One {
                        variant: format!("{:?}", *synapse),
                        soma: match requirement {
                            &Requirement::MetOne(ref uuid) => *uuid,
                            _ => panic!("axon failed to validate"),
                        },
                    },
                    &Constraint::Variadic(_) => ConstraintData::Variadic {
                        variant: format!("{:?}", *synapse),
                        somas: match requirement {
                            &Requirement::MetVariadic(ref somas) => {
                                somas.clone()
                            },
                            _ => unreachable!(),
                        },
                    },
                }
            })
            .collect();
        let dendrites = self.dendrites
            .iter()
            .map(|(synapse, &(ref constraint, ref requirement))| {
                match constraint {
                    &Constraint::One(_) => ConstraintData::One {
                        variant: format!("{:?}", *synapse),
                        soma: match requirement {
                            &Requirement::MetOne(ref uuid) => *uuid,
                            _ => panic!("axon failed to validate"),
                        },
                    },
                    &Constraint::Variadic(_) => ConstraintData::Variadic {
                        variant: format!("{:?}", *synapse),
                        somas: match requirement {
                            &Requirement::MetVariadic(ref somas) => {
                                somas.clone()
                            },
                            _ => unreachable!(),
                        },
                    },
                }
            })
            .collect();

        let uuid = self.uuid.unwrap();

        Ok((
            self,
            SomaData::Axon {
                terminals: terminals,
                dendrites: dendrites,
                uuid: uuid,
                name: unsafe { intrinsics::type_name::<Self>().to_string() },
            },
        ))
    }

    #[async(boxed)]
    fn update(mut self, imp: Impulse<T::Synapse>) -> Result<Self> {
        match imp {
            Impulse::AddDendrite(uuid, synapse, _) => {
                self.add_dendrite(uuid, synapse)?;

                self.soma =
                    await!(self.soma.update(imp)).map_err(|e| e.into())?;

                Ok(self)
            },
            Impulse::AddTerminal(uuid, synapse, _) => {
                self.add_terminal(uuid, synapse)?;

                self.soma =
                    await!(self.soma.update(imp)).map_err(|e| e.into())?;

                Ok(self)
            },
            Impulse::Start(uuid, _, _) => {
                self.start(uuid)?;

                self.soma =
                    await!(self.soma.update(imp)).map_err(|e| e.into())?;

                Ok(self)
            },

            Impulse::Probe(settings, tx) => {
                await!(self.perform_probe(settings, tx))
            },

            Impulse::Stop | Impulse::Error(_) => {
                bail!("unexpected impulse in axon")
            },
            //_ => await!(self.soma.update(imp))?,
        }
    }
}
