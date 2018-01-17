use std;
use std::collections::HashMap;

use super::{Effector, Error, Handle, Impulse, Result, Signal, Soma, Synapse};

/// defines dendrites on how connections can be made
#[derive(Debug, Copy, Clone)]
pub enum Dendrite<Y: Synapse> {
    /// require one connection with the specified role
    RequireOne(Y),

    /// require any number of connections with the specified role
    Variadic(Y),
}

enum DendriteHandle {
    One(Handle),
    Many(Vec<Handle>),
    Empty,
}

type DendriteMap<Y> = HashMap<Y, (DendriteHandle, Dendrite<Y>)>;

/// provides core convenience functions with little boilerplate
pub struct Axon<S: Signal, Y: Synapse> {
    effector: Option<Effector<S, Y>>,

    inputs: DendriteMap<Y>,
    outputs: DendriteMap<Y>,
}

impl<S: Signal, Y: Synapse> Axon<S, Y> {
    /// new axon with dendrites and default user-defined state
    pub fn new(
        inputs: Vec<Dendrite<Y>>,
        outputs: Vec<Dendrite<Y>>,
    ) -> Result<Self> {
        Ok(Self {
            effector: None,

            inputs: Self::create_roles(inputs)?,
            outputs: Self::create_roles(outputs)?,
        })
    }

    fn init(&mut self, effector: Effector<S, Y>) -> Result<()> {
        if self.effector.is_none() {
            self.effector = Some(effector);

            Ok(())
        } else {
            bail!("init called twice")
        }
    }

    fn add_input(&mut self, input: Handle, role: Y) -> Result<()> {
        Self::add_role(&mut self.inputs, input, role)
    }

    fn add_output(&mut self, output: Handle, role: Y) -> Result<()> {
        Self::add_role(&mut self.outputs, output, role)
    }

    fn verify(&self) -> Result<()> {
        if self.effector.is_none() {
            bail!("init was never called");
        }

        Self::verify_dendrites(&self.inputs)?;
        Self::verify_dendrites(&self.outputs)?;

        Ok(())
    }

    /// update the axon's inputs and outputs, then verify dendrites
    ///
    /// if axon handles the given message, it consumes it, otherwise it is
    /// returned so that the soma can use it.
    pub fn update(
        &mut self,
        msg: Impulse<S, Y>,
    ) -> Result<Option<Impulse<S, Y>>> {
        match msg {
            Impulse::Init(effector) => {
                self.init(effector)?;
                Ok(None)
            },

            Impulse::AddInput(input, role) => {
                self.add_input(input, role)?;
                Ok(None)
            },
            Impulse::AddOutput(output, role) => {
                self.add_output(output, role)?;
                Ok(None)
            },
            Impulse::Start => {
                self.verify()?;
                Ok(Some(Impulse::Start))
            },

            msg @ _ => Ok(Some(msg)),
        }
    }

    /// get the effector assigned to this soma
    pub fn effector(&self) -> Result<&Effector<S, Y>> {
        if self.effector.is_some() {
            Ok(self.effector.as_ref().unwrap())
        } else {
            bail!(concat!(
                "effector has not been set ",
                "(hint: state needs to be updated first)"
            ))
        }
    }
    /// convenience function for sending messages by role
    pub fn send_req_input(&self, dest: Y, msg: S) -> Result<()> {
        let req_input = self.req_input(dest)?;

        self.effector()?.send(req_input, msg);

        Ok(())
    }

    /// convenience function for sending messages by role
    pub fn send_req_output(&self, dest: Y, msg: S) -> Result<()> {
        let req_output = self.req_output(dest)?;

        self.effector()?.send(req_output, msg);

        Ok(())
    }

    /// get a RequireOne input
    pub fn req_input(&self, role: Y) -> Result<Handle> {
        Self::get_req(&self.inputs, role)
    }

    /// get a Variadic input
    pub fn var_input(&self, role: Y) -> Result<&Vec<Handle>> {
        Self::get_var(&self.inputs, role)
    }

    /// get a RequireOne output
    pub fn req_output(&self, role: Y) -> Result<Handle> {
        Self::get_req(&self.outputs, role)
    }

    /// get a Variadic output
    pub fn var_output(&self, role: Y) -> Result<&Vec<Handle>> {
        Self::get_var(&self.outputs, role)
    }

    fn create_roles(dendrites: Vec<Dendrite<Y>>) -> Result<DendriteMap<Y>> {
        let mut map = HashMap::new();

        for c in dendrites {
            let result = match c {
                Dendrite::RequireOne(role) => map.insert(
                    role,
                    (DendriteHandle::Empty, Dendrite::RequireOne(role)),
                ),
                Dendrite::Variadic(role) => map.insert(
                    role,
                    (DendriteHandle::Many(vec![]), Dendrite::Variadic(role)),
                ),
            };

            if result.is_some() {
                bail!("role {:?} specified more than once")
            }
        }

        Ok(map)
    }

    fn add_role(map: &mut DendriteMap<Y>, soma: Handle, role: Y) -> Result<()> {
        if let Some(&mut (ref mut handle, ref dendrite)) = map.get_mut(&role) {
            match *dendrite {
                Dendrite::RequireOne(role) => {
                    let new_hdl = match handle {
                        &mut DendriteHandle::Empty => DendriteHandle::One(soma),

                        _ => bail!(
                            "only one soma can be assigned to role {:?}",
                            role
                        ),
                    };

                    *handle = new_hdl;
                },
                Dendrite::Variadic(role) => match handle {
                    &mut DendriteHandle::Many(ref mut somas) => {
                        somas.push(soma);
                    },

                    _ => unreachable!("role {:?} was configured wrong", role),
                },
            };

            Ok(())
        } else {
            bail!("unexpected role {:?}", role)
        }
    }

    fn verify_dendrites(map: &DendriteMap<Y>) -> Result<()> {
        for (_, &(ref handle, ref dendrite)) in map.iter() {
            match *dendrite {
                Dendrite::RequireOne(role) => match handle {
                    &DendriteHandle::One(_) => (),
                    _ => bail!(
                        "role {:?} does not meet dendrite {:?}",
                        role,
                        *dendrite
                    ),
                },
                Dendrite::Variadic(_) => (),
            }
        }

        Ok(())
    }

    fn get_req(map: &DendriteMap<Y>, role: Y) -> Result<Handle> {
        if let Some(&(ref handle, Dendrite::RequireOne(_))) = map.get(&role) {
            match handle {
                &DendriteHandle::One(ref soma) => Ok(*soma),
                _ => bail!("role {:?} does not meet dendrite", role),
            }
        } else {
            bail!("unexpected role {:?}", role)
        }
    }

    fn get_var(map: &DendriteMap<Y>, role: Y) -> Result<&Vec<Handle>> {
        if let Some(&(ref handle, Dendrite::Variadic(_))) = map.get(&role) {
            match handle {
                &DendriteHandle::Many(ref somas) => Ok(somas),
                _ => unreachable!("role {:?} was configured wrong"),
            }
        } else {
            bail!("unexpected role {:?}", role)
        }
    }
}

/// soma used to wrap a Axon and a soma specialized with Neuron
pub struct Sheath<N>
where
    N: Neuron + Sized + 'static,
{
    axon: Axon<N::Signal, N::Synapse>,
    nucleus: N,
}

impl<N> Sheath<N>
where
    N: Neuron + Sized + 'static,
{
    /// wrap a nucleus and constrain the axon
    pub fn new(
        nucleus: N,
        inputs: Vec<Dendrite<N::Synapse>>,
        outputs: Vec<Dendrite<N::Synapse>>,
    ) -> Result<Self> {
        Ok(Self {
            axon: Axon::new(inputs, outputs)?,
            nucleus: nucleus,
        })
    }
}

impl<N> Soma for Sheath<N>
where
    N: Neuron,
{
    type Signal = N::Signal;
    type Synapse = N::Synapse;
    type Error = N::Error;

    fn update(
        mut self,
        msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> std::result::Result<Self, Self::Error> {
        if let Some(msg) = self.axon.update(msg)? {
            let nucleus = self.nucleus.update(&self.axon, msg)?;

            Ok(Sheath {
                axon: self.axon,
                nucleus: nucleus,
            })
        } else {
            Ok(self)
        }
    }
}

/// a specialized soma meant to ensure the Axon is always handled correctly
pub trait Neuron: Sized {
    /// a message that was not handled by the Axon
    type Signal: Signal;
    /// the role a connection between somas takes
    type Synapse: Synapse;
    /// error that occurs when an update fails
    type Error: std::error::Error + Send + From<Error> + 'static;

    /// update the nucleus with the Axon and soma message
    fn update(
        self,
        axon: &Axon<Self::Signal, Self::Synapse>,
        msg: Impulse<Self::Signal, Self::Synapse>,
    ) -> std::result::Result<Self, Self::Error>;
}
