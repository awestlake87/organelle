//#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate organelle;
extern crate tokio_core;

use futures::prelude::*;
use organelle::*;
use tokio_core::reactor;

struct ProbeControllerSoma;

impl ProbeControllerSoma {
    fn sheath() -> Result<Sheath<Self>> {
        Sheath::new(
            Self {},
            vec![],
            vec![Dendrite::RequireOne(ProbeSynapse::ProbeController)],
        )
    }
}

impl Neuron for ProbeControllerSoma {
    type Signal = ProbeSignal;
    type Synapse = ProbeSynapse;
    type Error = Error;

    fn update(
        self,
        axon: &Axon<ProbeSignal, ProbeSynapse>,
        imp: Impulse<ProbeSignal, ProbeSynapse>,
    ) -> Result<Self> {
        match imp {
            Impulse::Start => {
                axon.send_req_output(
                    ProbeSynapse::ProbeController,
                    ProbeSignal::RequestProbe,
                )?;
                Ok(self)
            },
            _ => Ok(self),
        }
    }
}

struct Placeholder1;

impl Soma for Placeholder1 {
    type Signal = ProbeSignal;
    type Synapse = ProbeSynapse;
    type Error = Error;

    fn update(self, _: Impulse<ProbeSignal, ProbeSynapse>) -> Result<Self> {
        Ok(self)
    }
}

struct Placeholder2;

impl Soma for Placeholder2 {
    type Signal = ProbeSignal;
    type Synapse = ProbeSynapse;
    type Error = Error;

    fn update(self, _: Impulse<ProbeSignal, ProbeSynapse>) -> Result<Self> {
        Ok(self)
    }
}

#[test]
fn probe() {
    let mut core = reactor::Core::new().unwrap();
    let mut organelle =
        Organelle::new(core.handle(), ProbeControllerSoma::sheath().unwrap());

    let controller = organelle.get_main_handle();
    let probe = organelle.add_soma(ProbeSoma::new());

    let mut sub = Organelle::new(core.handle(), Placeholder1 {});

    sub.add_soma(Placeholder2 {});
    organelle.add_soma(sub);

    organelle.connect(controller, probe, ProbeSynapse::ProbeController);

    core.run(organelle.into_future()).unwrap();
}
