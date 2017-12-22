#![warn(missing_docs)]

//! Cortical - general purpose reactive lobe networks

#[macro_use]
extern crate error_chain;

extern crate futures;
extern crate tokio;
extern crate tokio_core;
extern crate uuid;

use std::collections::{ HashMap };
use std::mem;
use std::rc::Rc;

use futures::prelude::*;
use futures::sync::{ mpsc };
use tokio_core::reactor;
use uuid::Uuid;

/// cortical error
error_chain! {
    errors {
        /// a lobe returned an error when called into
        LobeError {
            description("an error occurred while calling into a lobe"),
            display("an error occurred while calling into a lobe")
        }
    }
}

/// handle to a lobe within the cortex
pub type Handle = Uuid;

/// reactive structure designed perform any work requested by lobes
///
/// relays messages between lobes asynchronously, provides lobes with a list
/// of their inputs and outputs, and exposes a effects system to perform
/// arbitrary asio work
///
/// generic across a Message type M, however it is not limited to this global
/// type. as long as another message type can convert Into and From M, then it
/// can be delivered through this structure with absolutely no boilerplate.
pub struct Cortex<M> {
    core:       reactor::Core,
    sender:     mpsc::Sender<Protocol<M>>,
    receiver:   mpsc::Receiver<Protocol<M>>,

    nodes:      HashMap<Handle, Box<Node<M>>>,
}

/// a set of protocol messages to be relayed throughout the network
///
/// wraps a user-defined message within the cortex protocol.
/// 1. a lobe is always updated with Init first.
/// 2. as the cortex is built, the lobe will be updated with any inputs or
///     outputs specified using AddInput and AddOutput.
/// 3. when the cortex is ready to begin execution, every lobe is updated with
///     Start
/// 4. any messages sent between lobes will come through Message
/// 5. when a lobe determines that the cortex should stop, it can issue Stop
///     and the cortex will exit its event loop.
pub enum Protocol<M> {
    /// initializes a lobe with an effector to use
    Init(Effector<M>),
    /// notifies lobe that it has an input specified by Handle
    AddInput(Handle),
    /// notifies lobe that it has an output specified by Handle
    AddOutput(Handle),

    /// notifies lobe that cortex has begun execution
    Start,

    /// internal use only - used to track source and destination of message
    Payload(Handle, Handle, M),

    /// updates the lobe with a user-defined message from source lobe Handle
    Message(Handle, M),

    /// tells the cortex to stop executing
    Stop,
}

impl<M> Protocol<M> {
    fn convert_protocol<T>(msg: Protocol<T>) -> Self
        where
            M: From<T> + Into<T> + 'static,
            T: From<M> + Into<M> + 'static,
    {
        match msg {
            Protocol::Init(effector) => {
                let sender = effector.sender;

                Protocol::Init(
                    Effector {
                        handle: effector.handle,
                        sender: Rc::from(
                            move |effector: &reactor::Handle, msg: Protocol<M>| sender(
                                effector,
                                Protocol::<T>::convert_protocol(
                                    msg
                                )
                            )
                        ),
                        reactor: effector.reactor,
                    }
                )
            },

            Protocol::AddInput(input) => Protocol::AddInput(input),
            Protocol::AddOutput(output) => Protocol::AddOutput(
                output
            ),

            Protocol::Start => Protocol::Start,

            Protocol::Payload(src, dest, msg) => Protocol::Payload(
                src, dest, msg.into()
            ),
            Protocol::Message(src, msg) => Protocol::Message(
                src, msg.into()
            ),

            Protocol::Stop => Protocol::Stop,
        }
    }
}

/// the effector is a lobe's method of communicating between other lobes
///
/// the effector can send a message to any destination, provided you have its
/// handle. it will route these messages asynchronously to their destination,
/// so communication can be tricky, however, this is truly the best way I've
/// found to compose efficient, scalable, and potentially multithreaded
/// systems.
pub struct Effector<M> {
    handle:     Handle,
    sender:     Rc<Fn(&reactor::Handle, Protocol<M>)>,
    reactor:    reactor::Handle,
}

impl<M> Effector<M> where M: 'static {
    /// get the Handle associated with the lobe that owns this effector
    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// send a message to dest lobe
    pub fn send(&self, dest: Handle, msg: M) {
        self.send_cortex_message(
            Protocol::Payload(self.handle(), dest, msg)
        );
    }

    /// stop the cortex
    pub fn stop(&self) {
        self.send_cortex_message(Protocol::Stop);
    }

    fn send_cortex_message(&self, msg: Protocol<M>) {
        (*self.sender)(&self.reactor, msg);
    }
}

impl<M> Cortex<M> where M: 'static {
    /// create a new cortex
    pub fn new() -> Self {
        let (queue_tx, queue_rx) = mpsc::channel(100);

        Self {
            core: reactor::Core::new().unwrap(),
            sender: queue_tx,
            receiver: queue_rx,

            nodes: HashMap::new()
        }
    }

    /// add a new lobe to the cortex and initialize it
    ///
    /// as long as the lobe's message type can convert Into and From the
    /// cortex's message type, it can be added to the cortex and can
    /// communicate with any lobes that do the same.
    pub fn add_lobe<L, T>(&mut self, lobe: L) -> Handle where
        L: Lobe<Message=T> + 'static,
        M: From<T> + Into<T> + 'static,
        T: From<M> + Into<M> + 'static,
    {
        let mut node = Box::new(LobeWrapper::new(lobe));
        let handle = Handle::new_v4();

        let sender = self.sender.clone();

        node.update(
            Protocol::Init(
                Effector {
                    handle: handle,
                    sender: Rc::from(
                        move |r: &reactor::Handle, msg| r.spawn(
                             sender.clone().send(msg)
                                .then(
                                    |result| match result {
                                        Ok(_) => Ok(()),
                                        Err(_) => Ok(())
                                    }
                                )
                        )
                    ),
                    reactor: self.core.handle(),
                }
            )
        );

        self.nodes.insert(handle, node);

        handle
    }

    /// connect input to output and update them accordingly
    pub fn connect(&mut self, input: Handle, output: Handle) {
        self.nodes.get_mut(&input).unwrap().update(
            Protocol::AddOutput(output)
        );
        self.nodes.get_mut(&output).unwrap().update(
            Protocol::AddInput(input)
        );
    }

    /// run the cortex until it encounters the Stop command
    pub fn run(mut self) -> Result<()> {
        for ref mut node in self.nodes.values_mut() {
            node.update(Protocol::Start);
        }

        let mut nodes = self.nodes;

        let stream_future = self.receiver.take_while(
            |msg| match *msg {
                Protocol::Stop => {
                    println!("ended cleanly");
                    Ok(false)
                },
                _ => Ok(true)
            }
        ).for_each(
            move |msg| match msg {
                Protocol::Payload(src, dest, msg) => {
                    nodes.get_mut(&dest).unwrap().update(
                        Protocol::Message(src, msg)
                    );

                    Ok(())
                },

                Protocol::Stop => Ok(()),

                _ => unreachable!(),
            }
        );

        self.core.run(
            stream_future
                .map(|_| ())
                .map_err(|_| -> Error {
                    ErrorKind::Msg("whoops!".into()).into()
                })
        )?;

        Ok(())
    }
}

trait Node<M> {
    fn update(&mut self, msg: Protocol<M>);
}

struct LobeWrapper<L>(Option<L>);

impl<L> LobeWrapper<L> {
    fn new(lobe: L) -> Self {
        LobeWrapper::<L>(Some(lobe))
    }
}

impl<L, I, O> Node<O> for LobeWrapper<L> where
    L: Lobe<Message=I>,
    I: From<O> + Into<O> + 'static,
    O: From<I> + Into<I> + 'static
{
    fn update(&mut self, msg: Protocol<O>) {
        let lobe = mem::replace(&mut self.0, None)
            .unwrap()
            .update(Protocol::<I>::convert_protocol(msg))
        ;

        self.0 = Some(lobe);
    }
}

/// defines an interface for a lobe of any type
pub trait Lobe: Sized {
    /// the user-defined message to be passed between lobes
    type Message;

    /// apply any changes to the lobe's state as a result of _msg
    fn update(self, _msg: Protocol<Self::Message>) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    struct IncrementerLobe {
        effector: Option<Effector<IncrementerMessage>>,

        output: Option<Handle>,
    }

    impl IncrementerLobe {
        fn new() -> Self {
            Self {
                effector: None,
                output: None,
            }
        }

        fn effector(&self) -> &Effector<IncrementerMessage> {
            self.effector.as_ref().unwrap()
        }
    }

    impl Lobe for IncrementerLobe {
        type Message = IncrementerMessage;

        fn update(mut self, msg: Protocol<Self::Message>) -> Self {
            match msg {
                Protocol::Init(effector) => {
                    println!("incrementer initialized: {}", effector.handle());
                    self.effector = Some(effector);
                },
                Protocol::AddOutput(output) => {
                    println!("incrementer output: {}", output);
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

            self
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

    struct CounterLobe {
        effector: Option<Effector<CounterMessage>>,

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

        fn effector(&self) -> &Effector<CounterMessage> {
            self.effector.as_ref().unwrap()
        }
    }

    impl Lobe for CounterLobe {
        type Message = CounterMessage;

        fn update(mut self, msg: Protocol<Self::Message>) -> Self {
            match msg {
                Protocol::Init(effector) => {
                    println!("counter initialized: {}", effector.handle());
                    self.effector = Some(effector);
                },
                Protocol::AddInput(input) => {
                    println!("counter input: {}", input);
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

            self
        }
    }

    #[test]
    fn test_cortex() {
        let mut cortex = Cortex::<IncrementerMessage>::new();

        let lobe1 = cortex.add_lobe(IncrementerLobe::new());
        let lobe2 = cortex.add_lobe(CounterLobe::new());

        cortex.connect(lobe1, lobe2);

        cortex.run().unwrap();
    }
}
