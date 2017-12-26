
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use super::{ Result, Protocol, Effector, Handle };

/// defines constraints on how connections can be made
#[derive(Debug, Copy, Clone)]
pub enum Constraint<R> where
    R: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
    /// require one connection with the specified role
    RequireOne(R),

    /// require any number of connections with the specified role
    Variadic(R),
}

enum ConstraintHandle {
    One(Handle),
    Many(Vec<Handle>),
    Empty,
}

type ConstraintMap<R> = HashMap<R, (ConstraintHandle, Constraint<R>)>;

/// provides core convenience functions with little boilerplate
pub struct Soma<M, R> where
    M: 'static,
    R: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
    effector:               Option<Effector<M, R>>,

    inputs:                 ConstraintMap<R>,
    outputs:                ConstraintMap<R>,
}

impl<M, R> Soma<M, R> where
    M: 'static,
    R: Debug + Copy + Clone + Hash + Eq + PartialEq + 'static,
{
    /// new soma with constraints and default user-defined state
    pub fn new(inputs: Vec<Constraint<R>>, outputs: Vec<Constraint<R>>)
        -> Result<Self>
    {
        Ok(
            Self {
                effector: None,

                inputs: Self::create_roles(inputs)?,
                outputs: Self::create_roles(outputs)?,
            }
        )
    }

    fn init(&mut self, effector: Effector<M, R>) -> Result<()> {
        if self.effector.is_none() {
            self.effector = Some(effector);

            Ok(())
        }
        else {
            bail!("init called twice")
        }
    }

    fn add_input(&mut self, input: Handle, role: R) -> Result<()> {
        Self::add_role(&mut self.inputs, input, role)
    }

    fn add_output(&mut self, output: Handle, role: R) -> Result<()> {
        Self::add_role(&mut self.outputs, output, role)
    }

    fn verify(&self) -> Result<()> {
        if self.effector.is_none() {
            bail!("init was never called");
        }

        Self::verify_constraints(&self.inputs)?;
        Self::verify_constraints(&self.outputs)?;

        Ok(())
    }

    /// update the soma's inputs and outputs, then verify constraints
    pub fn update(&mut self, msg: &Protocol<M, R>) -> Result<()> {
        match msg {
            &Protocol::Init(ref effector) => self.init(effector.clone()),

            &Protocol::AddInput(ref input, ref role) => {
                self.add_input(*input, *role)
            },
            &Protocol::AddOutput(ref output, ref role) => {
                self.add_output(*output, *role)
            },

            &Protocol::Start => {
                self.verify()
            },

            _ => Ok(())
        }
    }

    /// get the effector assigned to this lobe
    pub fn effector(&self) -> Result<&Effector<M, R>> {
        if self.effector.is_some() {
            Ok(self.effector.as_ref().unwrap())
        }
        else {
            bail!(
                concat!(
                    "effector has not been set ",
                    "(hint: state needs to be updated first)"
                )
            )
        }
    }
    /// convenience function for sending messages by role
    pub fn send_req_input(&self, dest: R, msg: M) -> Result<()> {
        let req_input = self.req_input(dest)?;

        self.effector()?.send(req_input, msg);

        Ok(())
    }

    /// convenience function for sending messages by role
    pub fn send_req_output(&self, dest: R, msg: M) -> Result<()> {
        let req_output = self.req_output(dest)?;

        self.effector()?.send(req_output, msg);

        Ok(())
    }

    /// get a RequireOne input
    pub fn req_input(&self, role: R) -> Result<Handle> {
        Self::get_req(&self.inputs, role)
    }

    /// get a Variadic input
    pub fn var_input(&self, role: R) -> Result<&Vec<Handle>> {
        Self::get_var(&self.inputs, role)
    }

    /// get a RequireOne output
    pub fn req_output(&self, role: R) -> Result<Handle> {
        Self::get_req(&self.outputs, role)
    }

    /// get a Variadic output
    pub fn var_output(&self, role: R) -> Result<&Vec<Handle>> {
        Self::get_var(&self.outputs, role)
    }

    fn create_roles(constraints: Vec<Constraint<R>>)
        -> Result<ConstraintMap<R>>
    {
        let mut map = HashMap::new();

        for c in constraints {
            let result = match c {
                Constraint::RequireOne(role) => map.insert(
                    role,
                    (ConstraintHandle::Empty, Constraint::RequireOne(role))
                ),
                Constraint::Variadic(role) => map.insert(
                    role,
                    (
                        ConstraintHandle::Many(vec![ ]),
                        Constraint::Variadic(role)
                    )
                )
            };

            if result.is_some() {
                bail!("role {:?} specified more than once")
            }
        }

        Ok(map)
    }

    fn add_role(map: &mut ConstraintMap<R>, lobe: Handle, role: R)
        -> Result<()>
    {
        if let Some(&mut (ref mut handle, ref constraint))
            = map.get_mut(&role)
        {
            match *constraint {
                Constraint::RequireOne(role) => {
                    let new_hdl = match handle {
                        &mut ConstraintHandle::Empty => {
                            ConstraintHandle::One(lobe)
                        },

                        _ => bail!(
                            "only one lobe can be assigned to role {:?}",
                            role
                        ),
                    };

                    *handle = new_hdl;
                },
                Constraint::Variadic(role) => match handle {
                    &mut ConstraintHandle::Many(ref mut lobes) => {
                        lobes.push(lobe);
                    },

                    _ => unreachable!("role {:?} was configured wrong", role)
                }
            };

            Ok(())
        }
        else {
            bail!("unexpected role {:?}", role)
        }
    }

    fn verify_constraints(map: &ConstraintMap<R>) -> Result<()> {
        for (_, &(ref handle, ref constraint)) in map.iter() {
            match *constraint {
                Constraint::RequireOne(role) => match handle {
                    &ConstraintHandle::One(_) => (),
                    _ => bail!(
                        "role {:?} does not meet constraint {:?}",
                        role,
                        *constraint
                    )
                },
                Constraint::Variadic(_) => (),
            }
        }

        Ok(())
    }

    fn get_req(map: &ConstraintMap<R>, role: R) -> Result<Handle> {
        if let Some(&(ref handle, Constraint::RequireOne(_))) = map.get(&role)
        {
            match handle {
                &ConstraintHandle::One(ref lobe) => Ok(*lobe),
                _ => bail!("role {:?} does not meet constraint", role)
            }
        }
        else {
            bail!("unexpected role {:?}", role)
        }
    }

    fn get_var(map: &ConstraintMap<R>, role: R) -> Result<&Vec<Handle>> {
        if let Some(&(ref handle, Constraint::Variadic(_))) = map.get(&role) {
            match handle {
                &ConstraintHandle::Many(ref lobes) => Ok(lobes),
                _ => unreachable!("role {:?} was configured wrong")
            }
        }
        else {
            bail!("unexpected role {:?}", role)
        }
    }
}
