use futures::prelude::*;

use super::{Error, Impulse, Result, Soma};

/// constraints that can be put on axons for validation purposes
pub enum Dendrite<R> {
    /// only accept one synapse for the given role
    One(R),
}

/// wrap a soma with a set of requirements that will be validated upon startup
pub struct Axon<T: Soma + 'static> {
    soma: T,
    inputs: Vec<Dendrite<T::Role>>,
    outputs: Vec<Dendrite<T::Role>>,
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
            inputs: inputs,
            outputs: outputs,
        }
    }

    fn add_input(&self, role: T::Role) -> Result<()> {
        println!("add input role {:?}", role);
        Ok(())
    }

    fn add_output(&self, role: T::Role) -> Result<()> {
        println!("add output role {:?}", role);
        Ok(())
    }

    fn start(&self) -> Result<()> {
        println!("axon validate");
        Ok(())
    }
}

impl<T: Soma + 'static> Soma for Axon<T> {
    type Role = T::Role;
    type Synapse = T::Synapse;
    type Error = Error;
    type Future = Box<Future<Item = Self, Error = Self::Error>>;

    #[async(boxed)]
    fn update(self, imp: Impulse<T::Role, T::Synapse>) -> Result<Self> {
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
