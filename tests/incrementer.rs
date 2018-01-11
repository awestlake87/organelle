
#[macro_use]
extern crate error_chain;

extern crate organelle;

use organelle::*;

#[derive(Debug)]
enum IncrementerMessage {
    Increment,
    Ack,
}

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

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum IncrementerRole {
    Incrementer,
    Counter,
}

impl From<CounterRole> for IncrementerRole {
    fn from(c: CounterRole) -> IncrementerRole {
        match c {
            CounterRole::Incrementer => IncrementerRole::Incrementer,
            CounterRole::Counter => IncrementerRole::Counter,
        }
    }
}

type IncrementerSoma = Soma<IncrementerMessage, IncrementerRole>;

struct IncrementerCell {
    soma:       IncrementerSoma,
}

impl IncrementerCell {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: IncrementerSoma::new(
                    vec![ ],
                    vec![
                        Constraint::RequireOne(IncrementerRole::Incrementer)
                    ]
                )?,
            }
        )
    }
}

impl Cell for IncrementerCell {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Protocol::Start => {
                    self.soma.send_req_output(
                        IncrementerRole::Incrementer,
                        IncrementerMessage::Increment
                    )?;
                },

                Protocol::Message(_, IncrementerMessage::Ack) => {
                    self.soma.send_req_output(
                        IncrementerRole::Incrementer,
                        IncrementerMessage::Increment
                    )?;
                },

                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

#[derive(Debug)]
enum CounterMessage {
    BumpCounter,
    Ack,
}

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

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
enum CounterRole {
    Incrementer,
    Counter,
}

impl From<IncrementerRole> for CounterRole {
    fn from(role: IncrementerRole) -> CounterRole {
        match role {
            IncrementerRole::Incrementer => CounterRole::Incrementer,
            IncrementerRole::Counter => CounterRole::Counter,
        }
    }
}

type CounterSoma = Soma<CounterMessage, CounterRole>;

struct CounterCell {
    soma: CounterSoma,

    counter: u32
}

impl CounterCell {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: CounterSoma::new(
                    vec![ Constraint::RequireOne(CounterRole::Incrementer) ],
                    vec![ ]
                )?,

                counter: 0
            }
        )
    }
}

impl Cell for CounterCell {
    type Message = CounterMessage;
    type Role = CounterRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Protocol::Start => (),

                Protocol::Message(_, CounterMessage::BumpCounter) => {
                    if self.counter < 5 {
                        println!("counter increment");

                        self.counter += 1;
                        self.soma.send_req_input(
                            CounterRole::Incrementer, CounterMessage::Ack
                        )?;
                    }
                    else {
                        println!("stop");
                        self.soma.effector()?.stop();
                    }
                },

                _ => bail!("unexpected message"),
            }
        }

        Ok(self)
    }
}

struct ForwarderCell {
    soma: CounterSoma,
}

impl ForwarderCell {
    fn new() -> Result<Self> {
        Ok(
            Self {
                soma: CounterSoma::new(
                    vec![ Constraint::RequireOne(CounterRole::Incrementer) ],
                    vec![ Constraint::RequireOne(CounterRole::Incrementer) ],
                )?,
            }
        )
    }
}

impl Cell for ForwarderCell {
    type Message = CounterMessage;
    type Role = CounterRole;

    fn update(mut self, msg: Protocol<Self::Message, Self::Role>)
        -> Result<Self>
    {
        if let Some(msg) = self.soma.update(msg)? {
            match msg {
                Protocol::Start => (),

                Protocol::Message(src, msg) => {
                    if src == self.soma.req_input(CounterRole::Incrementer)? {
                        println!(
                            "forwarding input {:#?} through {}",
                            msg,
                            self.soma.effector()?.this_cell()
                        );

                        self.soma.send_req_output(
                            CounterRole::Incrementer, msg
                        )?;
                    }
                    else if
                        src == self.soma.req_output(CounterRole::Incrementer)?
                    {
                        println!(
                            "forwarding output {:#?} through {}",
                            msg,
                            self.soma.effector()?.this_cell()
                        );

                        self.soma.send_req_input(
                            CounterRole::Incrementer, msg
                        )?;
                    }
                },

                _ => bail!("unexpected message")
            }
        }

        Ok(self)
    }
}

#[test]
fn test_organelle() {
    let mut organelle = Organelle::new(IncrementerCell::new().unwrap());

    let counter = organelle.add_cell(CounterCell::new().unwrap());

    let main = organelle.get_main_handle();
    println!("organelle {}", main);
    organelle.connect(main, counter, IncrementerRole::Incrementer);

    organelle.run().unwrap();
}

#[test]
fn test_sub_organelle() {
    let mut counter_organelle = Organelle::new(ForwarderCell::new().unwrap());

    let forwarder = counter_organelle.get_main_handle();
    let counter = counter_organelle.add_cell(CounterCell::new().unwrap());

    counter_organelle.connect(forwarder, counter, CounterRole::Incrementer);

    let mut inc_organelle = Organelle::new(IncrementerCell::new().unwrap());

    let incrementer = inc_organelle.get_main_handle();
    let counter = inc_organelle.add_cell(counter_organelle);
    // connect the incrementer to the counter organelle
    inc_organelle.connect(
        incrementer, counter, IncrementerRole::Incrementer
    );

    inc_organelle.run().unwrap();
}

struct InitErrorCell {

}

impl InitErrorCell {
    fn new() -> Self {
        Self { }
    }
}

impl Cell for InitErrorCell {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(self, msg: Protocol<Self::Message, Self::Role>) -> Result<Self> {
        match msg {
            Protocol::Init(effector) => {
                effector.error("a cell error!".into());

                Ok(self)
            },

            _ => Ok(self),
        }
    }
}

struct UpdateErrorCell {

}

impl UpdateErrorCell {
    fn new() -> Self {
        Self { }
    }
}

impl Cell for UpdateErrorCell {
    type Message = IncrementerMessage;
    type Role = IncrementerRole;

    fn update(self, _: Protocol<Self::Message, Self::Role>) -> Result<Self> {
        bail!("update failed")
    }
}

#[test]
fn test_cell_error() {
    if let Ok(_) = InitErrorCell::new().run() {
        panic!("cell init was supposed to fail");
    }

    if let Ok(_) = UpdateErrorCell::new().run() {
        panic!("cell update was supposed to fail");
    }

    if let Ok(_) = Organelle::new(UpdateErrorCell { }).run() {
        panic!("organelle updates were supposed to fail");
    }
}
