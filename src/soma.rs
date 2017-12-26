
use super::{ Result, Protocol, Effector, Handle };

/// provides core convenience functions with little boilerplate
pub struct Soma<M, R> where
    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static,
{
    effector:               Option<Effector<M, R>>,
}

impl<M, R> Soma<M, R> where
    M: 'static,
    R: Copy + Clone + Eq + PartialEq + 'static,
{
    /// new soma with constraints and default user-defined state
    pub fn new() -> Self {
        Self {
            effector: None,
        }
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

    fn verify(&self) -> Result<()> {
        if self.effector.is_none() {
            bail!("init was never called");
        }

        Ok(())
    }

    /// update the soma's inputs and outputs, then verify constraints
    pub fn update(&mut self, msg: &Protocol<M, R>) -> Result<()> {
        match msg {
            &Protocol::Init(ref effector) => self.init(effector.clone()),

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

    /// convenience function for sending messages
    pub fn send(&self, dest: Handle, msg: M) -> Result<()> {
        self.effector()?.send(dest, msg);

        Ok(())
    }

    /// convenience function for stopping the cortex
    pub fn stop(&self) -> Result<()> {
        self.effector()?.stop();

        Ok(())
    }
}
