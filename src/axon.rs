use std::collections::HashMap;

use futures::prelude::*;

use super::{Error, ErrorKind, Impulse, Result, Soma};

/// constraints that can be put on axons for validation purposes
pub enum Dendrite<R> {
    /// only accept one synapse for the given role
    One(R),
    /// accept any number of synapses for the given role
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
    inputs: HashMap<T::Role, (Dendrite<T::Role>, Requirement)>,
    outputs: HashMap<T::Role, (Dendrite<T::Role>, Requirement)>,
}

impl<T: Soma + 'static> Axon<T> {
    /// wrap a soma with constraints specified by input and output dendrites
    pub fn new(
        soma: T,
        inputs: Vec<Dendrite<T::Role>>,
        outputs: Vec<Dendrite<T::Role>>,
    ) -> Self {
        Self {
            soma: soma,
            inputs: inputs
                .iter()
                .map(|d| match d {
                    &Dendrite::One(r) => {
                        (r, (Dendrite::One(r), Requirement::Unmet))
                    },
                    &Dendrite::Variadic(r) => {
                        (r, (Dendrite::Variadic(r), Requirement::Met))
                    },
                })
                .collect(),
            outputs: outputs
                .iter()
                .map(|d| match d {
                    &Dendrite::One(r) => {
                        (r, (Dendrite::One(r), Requirement::Unmet))
                    },
                    &Dendrite::Variadic(r) => {
                        (r, (Dendrite::Variadic(r), Requirement::Met))
                    },
                })
                .collect(),
        }
    }

    fn add_input(&mut self, role: T::Role) -> Result<()> {
        if let Some(&mut (ref mut dendrite, ref mut req)) =
            self.inputs.get_mut(&role)
        {
            match dendrite {
                &mut Dendrite::One(_) => match req {
                    &mut Requirement::Unmet => *req = Requirement::Met,
                    &mut Requirement::Met => bail!(ErrorKind::InvalidSynapse(
                        format!("expected only one input for {:?}", role)
                    )),
                },
                &mut Dendrite::Variadic(_) => (),
            }
        } else {
            bail!(ErrorKind::InvalidSynapse(format!(
                "no dendrites found for {:?}",
                role
            )))
        }

        Ok(())
    }

    fn add_output(&mut self, role: T::Role) -> Result<()> {
        if let Some(&mut (ref mut dendrite, ref mut req)) =
            self.outputs.get_mut(&role)
        {
            match dendrite {
                &mut Dendrite::One(_) => match req {
                    &mut Requirement::Unmet => *req = Requirement::Met,
                    &mut Requirement::Met => bail!(ErrorKind::InvalidSynapse(
                        format!("expected only one output for {:?}", role)
                    )),
                },
                &mut Dendrite::Variadic(_) => (),
            }
        } else {
            bail!(ErrorKind::InvalidSynapse(format!(
                "no dendrites found for {:?}",
                role
            )))
        }

        Ok(())
    }

    fn start(&self) -> Result<()> {
        for (role, &(ref dendrite, ref req)) in &self.inputs {
            match dendrite {
                &Dendrite::One(_) => match req {
                    &Requirement::Met => (),
                    &Requirement::Unmet => bail!(ErrorKind::MissingSynapse(
                        format!("expected input synapse for {:?}", *role)
                    )),
                },
                &Dendrite::Variadic(_) => assert_eq!(*req, Requirement::Met),
            }
        }

        for (role, &(ref dendrite, ref req)) in &self.outputs {
            match dendrite {
                &Dendrite::One(_) => match req {
                    &Requirement::Met => (),
                    &Requirement::Unmet => bail!(ErrorKind::MissingSynapse(
                        format!("expected output synapse for {:?}", *role)
                    )),
                },
                &Dendrite::Variadic(_) => assert_eq!(*req, Requirement::Met),
            }
        }

        Ok(())
    }
}

impl<T: Soma + 'static> Soma for Axon<T> {
    type Role = T::Role;
    type Synapse = T::Synapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(mut self, imp: Impulse<T::Role, T::Synapse>) -> Result<Self> {
        Ok(Self {
            soma: match imp {
                Impulse::AddInput(role, _) => {
                    self.add_input(role)?;

                    await!(self.soma.update(imp)).map_err(|e| e.into())?
                },
                Impulse::AddOutput(role, _) => {
                    self.add_output(role)?;

                    await!(self.soma.update(imp)).map_err(|e| e.into())?
                },
                Impulse::Start(_, _) => {
                    self.start()?;

                    await!(self.soma.update(imp)).map_err(|e| e.into())?
                },

                Impulse::Stop | Impulse::Error(_) => {
                    bail!("unexpected impulse in axon")
                },
                //_ => await!(self.soma.update(imp))?,
            },
            inputs: self.inputs,
            outputs: self.outputs,
        })
    }
}
