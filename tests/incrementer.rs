
#[macro_use]
extern crate error_chain;

extern crate cortical;

use cortical::*;

#[derive(Debug)]
enum IncrementerMessage {
    Increment,
    Ack,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum IncrementerConstraint {
    Incrementer,
    Forwarder,
    Counter,
}

type IncrementerEffector = Effector<
    IncrementerMessage, IncrementerConstraint
>;
type IncrementerProtocol = Protocol<
    IncrementerMessage, IncrementerConstraint
>;
type IncrementerCortex = Cortex<IncrementerMessage, IncrementerConstraint>;

impl From<CounterMessage> for IncrementerMessage {
    fn from(msg: CounterMessage) -> IncrementerMessage {
        match msg {
            CounterMessage::Ack => IncrementerMessage::Ack,
            msg @ _ => panic!(
                "counter does not support {:#?}", msg
            ),
        }
    }
}

impl From<CounterConstraint> for IncrementerConstraint {
    fn from(c: CounterConstraint) -> IncrementerConstraint {
        match c {
            CounterConstraint::Incrementer => {
                IncrementerConstraint::Incrementer
            },
            CounterConstraint::Forwarder => {
                IncrementerConstraint::Forwarder
            },
            CounterConstraint::Counter => IncrementerConstraint::Counter,
        }
    }
}

struct IncrementerLobe {
    effector: Option<IncrementerEffector>,

    output: Option<Handle>,
}

impl IncrementerLobe {
    fn new() -> Self {
        Self {
            effector: None,
            output: None,
        }
    }

    fn effector(&self) -> &IncrementerEffector {
        self.effector.as_ref().unwrap()
    }
}

impl Lobe for IncrementerLobe {
    type Message = IncrementerMessage;
    type Constraint = IncrementerConstraint;

    fn update(mut self, msg: IncrementerProtocol) -> Result<Self> {
        match msg {
            Protocol::Init(effector) => {
                println!("incrementer: {}", effector.handle());
                self.effector = Some(effector);
            },
            Protocol::AddOutput(output, constraint) => {
                println!(
                    "incrementer output {} {:#?}", output, constraint
                );

                assert!(
                    constraint == IncrementerConstraint::Incrementer
                    || constraint == IncrementerConstraint::Forwarder
                );

                self.output = Some(output);
            },

            Protocol::Start => {
                if let Some(output) = self.output {
                    self.effector().send(
                        output, IncrementerMessage::Increment
                    );
                }
                else {
                    self.effector().stop();
                }
            },

            Protocol::Message(src, IncrementerMessage::Ack) => {
                assert_eq!(src, self.output.unwrap());
                println!("ACK");

                self.effector().send(
                    self.output.unwrap(), IncrementerMessage::Increment
                );
            },

            _ => (),
        }

        Ok(self)
    }
}

#[derive(Debug)]
enum CounterMessage {
    BumpCounter,
    Ack,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum CounterConstraint {
    Incrementer,
    Forwarder,
    Counter,
}

type CounterEffector = Effector<CounterMessage, CounterConstraint>;
type CounterProtocol = Protocol<CounterMessage, CounterConstraint>;
type CounterCortex = Cortex<CounterMessage, CounterConstraint>;

impl From<IncrementerMessage> for CounterMessage {
    fn from(msg: IncrementerMessage) -> CounterMessage {
        match msg {
            IncrementerMessage::Increment => CounterMessage::BumpCounter,
            msg @ _ => panic!(
                "counter does not support {:#?}", msg
            ),
        }
    }
}

impl From<IncrementerConstraint> for CounterConstraint {
    fn from(constraint: IncrementerConstraint) -> CounterConstraint {
        match constraint {
            IncrementerConstraint::Incrementer => {
                CounterConstraint::Incrementer
            },
            IncrementerConstraint::Forwarder => {
                CounterConstraint::Forwarder
            },
            IncrementerConstraint::Counter => {
                CounterConstraint::Counter
            },
        }
    }
}

struct CounterLobe {
    effector: Option<CounterEffector>,

    input: Option<Handle>,

    counter: u32
}

impl CounterLobe {
    fn new() -> Self {
        Self {
            effector: None,
            input: None,
            counter: 0
        }
    }

    fn effector(&self) -> &CounterEffector {
        self.effector.as_ref().unwrap()
    }
}

impl Lobe for CounterLobe {
    type Message = CounterMessage;
    type Constraint = CounterConstraint;

    fn update(mut self, msg: CounterProtocol) -> Result<Self>
    {
        match msg {
            Protocol::Init(effector) => {
                println!("counter: {}", effector.handle());
                self.effector = Some(effector);
            },
            Protocol::AddInput(input, constraint) => {
                println!("counter input {} {:#?}", input, constraint);

                assert!(
                    constraint == CounterConstraint::Incrementer
                    || constraint == CounterConstraint::Forwarder
                );
                self.input = Some(input);
            },

            Protocol::Message(src, CounterMessage::BumpCounter) => {
                assert_eq!(src, self.input.unwrap());

                if self.counter < 5 {
                    println!("counter increment");

                    self.counter += 1;
                    self.effector().send(
                        self.input.unwrap(), CounterMessage::Ack
                    );
                }
                else {
                    println!("stop");
                    self.effector().stop();
                }
            },

            _ => (),
        }

        Ok(self)
    }
}

struct ForwarderLobe {
    effector: Option<Effector<CounterMessage, CounterConstraint>>,

    input: Option<Handle>,
    output: Option<Handle>,
}

impl ForwarderLobe {
    fn new() -> Self {
        Self { effector: None, input: None, output: None }
    }

    fn effector(&self) -> &Effector<CounterMessage, CounterConstraint> {
        self.effector.as_ref().unwrap()
    }
}

impl Lobe for ForwarderLobe {
    type Message = CounterMessage;
    type Constraint = CounterConstraint;

    fn update(mut self, msg: Protocol<Self::Message, Self::Constraint>)
        -> Result<Self>
    {
        match msg {
            Protocol::Init(effector) => {
                println!("forwarder: {}", effector.handle());
                self.effector = Some(effector);
            },
            Protocol::AddInput(input, constraint) => {
                assert!(
                    constraint == CounterConstraint::Incrementer
                    || constraint == CounterConstraint::Forwarder
                );

                println!("forwarder input: {}", input);
                self.input = Some(input);
            },
            Protocol::AddOutput(output, constraint) => {
                assert!(
                    constraint == CounterConstraint::Counter
                    || constraint == CounterConstraint::Forwarder
                );

                println!("forwarder output: {}", output);
                self.output = Some(output);
            },

            Protocol::Message(src, msg) => {
                if src == self.input.unwrap() {
                    println!(
                        "forwarding input {:#?} through {}",
                        msg,
                        self.effector().handle()
                    );

                    self.effector().send(self.output.unwrap(), msg);
                }
                else if src == self.output.unwrap() {
                    println!(
                        "forwarding output {:#?} through {}",
                        msg,
                        self.effector().handle()
                    );

                    self.effector().send(self.input.unwrap(), msg);
                }
            },

            _ => ()
        }

        Ok(self)
    }
}

#[test]
fn test_cortex() {
    let mut cortex = IncrementerCortex::new(
        IncrementerLobe::new(), CounterLobe::new()
    );

    let input = cortex.get_input();
    let output = cortex.get_output();
    cortex.connect(input, output, IncrementerConstraint::Incrementer);

    run(cortex).unwrap();
}

#[test]
fn test_sub_cortex() {
    let mut counter_cortex = CounterCortex::new(
        ForwarderLobe::new(), CounterLobe::new()
    );

    let counter_input = counter_cortex.get_input();
    let counter_output = counter_cortex.get_output();
    // connect the forwarder to the counter with the Forwarder constraint
    counter_cortex.connect(
        counter_input, counter_output, CounterConstraint::Forwarder
    );

    let mut inc_cortex = IncrementerCortex::new(
        IncrementerLobe::new(), counter_cortex
    );

    let inc_input = inc_cortex.get_input();
    let inc_output = inc_cortex.get_output();
    // connect the incrementer to the counter cortex
    inc_cortex.connect(
        inc_input, inc_output, IncrementerConstraint::Incrementer
    );

    run(inc_cortex).unwrap();
}

struct InitErrorLobe {

}

impl Lobe for InitErrorLobe {
    type Message = IncrementerMessage;
    type Constraint = IncrementerConstraint;

    fn update(self, msg: IncrementerProtocol)
        -> Result<Self>
    {
        match msg {
            Protocol::Init(effector) => {
                effector.error("a lobe error!".into());

                Ok(self)
            },

            _ => Ok(self),
        }
    }
}

struct UpdateErrorLobe {

}

impl Lobe for UpdateErrorLobe {
    type Message = IncrementerMessage;
    type Constraint = IncrementerConstraint;

    fn update(self, _: IncrementerProtocol)
        -> Result<Self>
    {
        bail!("update failed")
    }
}

#[test]
fn test_lobe_error() {
    if let Ok(_) = run(InitErrorLobe { }) {
        panic!("lobe init was supposed to fail");
    }

    if let Ok(_) = run(UpdateErrorLobe { }) {
        panic!("lobe update was supposed to fail");
    }

    if let Ok(_) = run(
        IncrementerCortex::new(UpdateErrorLobe { }, UpdateErrorLobe { })
    ) {
        panic!("cortex updates were supposed to fail");
    }
}
