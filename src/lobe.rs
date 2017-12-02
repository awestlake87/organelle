
use super::{ Result, NodeHdl };

/// a trait applied to a module that completes a sub-task in a cortex
pub trait Lobe {
    /// the input structure
    type Input;
    /// the output structure
    type Output;
    /// the feedback input structure
    type FeedbackInput;
    /// the feedback output structure
    type FeedbackOutput;

    /// called with the inital data
    fn start(
        &mut self,
        _this: NodeHdl,
        _inputs: Vec<NodeHdl>,
        _outputs: Vec<NodeHdl>
    )
        -> Result<()>
    {
        Ok(())
    }
    /// called periodically with updates
    fn update(&mut self, _input: Self::Input) -> Result<()> {
        Ok(())
    }
    /// called per connection to tailor output
    fn tailor_output(&mut self, output: NodeHdl) -> Result<Self::Output>;

    /// called in reverse for feedback
    fn feedback(&mut self, _input: Self::FeedbackInput) -> Result<()> {
        Ok(())
    }
    /// called per input to tailor feedback
    fn tailor_feedback(&mut self, input: NodeHdl)
        -> Result<Self::FeedbackOutput>
    ;

    /// called with the final data (start can be called again after this)
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}

#[macro_export]
macro_rules! create_lobe_data {
    {
        module: $module:ident,

        $(req $req_ident:ident: $req_type:ty,)*
        $(opt $opt_ident:ident: $opt_type:ty,)*
        $(var $var_ident:ident: $var_type:ty,)*

        $(out $out_ident:ident: $out_type:ty,)*

        $(fbk req $fbk_req_ident:ident: $fbk_req_type:ty,)*
        $(fbk opt $fbk_opt_ident:ident: $fbk_opt_type:ty,)*
        $(fbk var $fbk_var_ident:ident: $fbk_var_type:ty,)*

        $(fbk out $fbk_out_ident:ident: $fbk_out_type:ty,)*
    } => {
        mod $module {
            use super::*;

            #[allow(missing_docs)]
            pub struct Input {
                $(pub $req_ident: $req_type,)*
                $(pub $var_ident: Vec<$var_type>,)*
                $(pub $opt_ident: Option<$opt_type>,)*
            }

            #[allow(missing_docs)]
            pub struct Output {
                $(pub $out_ident: $out_type,)*
            }

            #[allow(missing_docs)]
            pub struct FeedbackInput {
                $(pub $fbk_req_ident: $fbk_req_type,)*
                $(pub $fbk_var_ident: Vec<$fbk_var_type>,)*
                $(pub $fbk_opt_ident: Option<$fbk_opt_type>,)*
            }

            #[allow(missing_docs)]
            pub struct FeedbackOutput {
                $(pub $fbk_out_ident: $fbk_out_type,)*
            }
        }
    }
}

#[macro_export]
macro_rules! constrain_lobe {
    {
        lobe: $lobe:ty,
        constraint: $constraint:ident,
        data: $data:ident,

        input: $input:ident,
        output: $output:ident,
        feedback_input: $feedback_input:ident,
        feedback_output: $feedback_output:ident,

        $(req $req_ident:ident: $req_const:ident,)*
        $(opt $opt_ident:ident: $opt_const:ident,)*
        $(var $var_ident:ident: $var_const:ident,)*

        $(out $out_ident:ident: $out_const:ident,)*

        $(fbk req $fbk_req_ident:ident: $fbk_req_const:ident,)*
        $(fbk opt $fbk_opt_ident:ident: $fbk_opt_const:ident,)*
        $(fbk var $fbk_var_ident:ident: $fbk_var_const:ident,)*

        $(fbk out $fbk_out_ident:ident: $fbk_out_const:ident,)*
    } => {
        impl $crate::FromCortexData<$data> for $input {
            #[allow(unused_variables)]
            fn from_cortex_data(data: $data) -> $crate::Result<$input> {
                Ok(
                    $input {
                        $(
                            $req_ident: match data.$req_const {
                                $crate::CortexLink::One(data) => data,
                                $crate::CortexLink::Many(_) => bail!(
                                    "{} expected only one of {}",
                                    stringify!($lobe),
                                    stringify!($req_const)
                                ),
                                $crate::CortexLink::Empty => bail!(
                                    "{} expected one of {}",
                                    stringify!($lobe),
                                    stringify!($req_const)
                                )
                            },
                        )*
                        $(
                            $var_ident: match data.$var_const {
                                $crate::CortexLink::One(data) => vec![
                                    data
                                ],
                                $crate::CortexLink::Many(data) => data,
                                $crate::CortexLink::Empty => vec![ ]
                            },
                        )*
                        $(
                            $opt_ident: match data.$opt_const {
                                $crate::CortexLink::One(data) => Some(
                                    data
                                ),
                                $crate::CortexLink::Many(_) => bail!(
                                    "{} expected either one or none of {}",
                                    stringify!($lobe),
                                    stringify!($opt_const)
                                ),
                                $crate::CortexLink::Empty => None,
                            },
                        )*
                    }
                )
            }
        }

        impl $crate::IntoCortexData<$data> for $output {
            #[allow(unused_variables)]
            fn into_cortex_data(self) -> $crate::Result<$data> {
                Ok(
                    $data {
                        $(
                            $out_const: $crate::CortexLink::One(
                                self.$out_ident
                            ),
                        )*
                        ..<$data as Default>::default()
                    }
                )
            }
        }

        impl $crate::FromCortexData<$data> for $feedback_input {
            #[allow(unused_variables)]
            fn from_cortex_data(data: $data)
                -> $crate::Result<$feedback_input>
            {
                Ok(
                    $feedback_input {
                        $(
                            $fbk_req_ident: match data.$fbk_req_const {
                                $crate::CortexLink::One(data) => data,
                                $crate::CortexLink::Many(_) => bail!(
                                    "{} expected only one of {}",
                                    stringify!($lobe),
                                    stringify!($fbk_req_const)
                                ),
                                $crate::CortexLink::Empty => bail!(
                                    "{} expected one of {}",
                                    stringify!($lobe),
                                    stringify!($fbk_req_const)
                                )
                            },
                        )*
                        $(
                            $fbk_var_ident: match data.$fbk_var_const {
                                $crate::CortexLink::One(data) => vec![
                                    data
                                ],
                                $crate::CortexLink::Many(data) => data,
                                $crate::CortexLink::Empty => vec![ ]
                            },
                        )*
                        $(
                            $fbk_opt_ident: match data.$fbk_opt_const {
                                $crate::CortexLink::One(data) => Some(
                                    data
                                ),
                                $crate::CortexLink::Many(_) => bail!(
                                    "{} expected either one or none of {}",
                                    stringify!($lobe),
                                    stringify!($fbk_opt_const)
                                ),
                                $crate::CortexLink::Empty => None,
                            },
                        )*
                    }
                )
            }
        }

        impl $crate::IntoCortexData<$data> for $feedback_output {
            #[allow(unused_variables)]
            fn into_cortex_data(self) -> $crate::Result<$data> {
                Ok(
                    $data {
                        $(
                            $fbk_out_const: $crate::CortexLink::One(
                                self.$fbk_out_ident
                            ),
                        )*
                        ..<$data as Default>::default()
                    }
                )
            }
        }

        impl $crate::CortexConstraints<$constraint> for $lobe {
            fn get_required_inputs(&self) -> Vec<$constraint> {
                vec![ $($constraint::$req_const,)* ]
            }
            fn get_variadic_inputs(&self) -> Vec<$constraint> {
                vec![ $($constraint::$var_const,)* ]
            }
            fn get_optional_inputs(&self) -> Vec<$constraint> {
                vec![ $($constraint::$opt_const,)* ]
            }

            fn get_outputs(&self) -> Vec<$constraint> {
                vec![ $($constraint::$out_const,)* ]
            }


            fn get_required_feedback(&self) -> Vec<$constraint> {
                vec![ $($constraint::$fbk_req_const,)* ]
            }
            fn get_variadic_feedback(&self) -> Vec<$constraint> {
                vec![ $($constraint::$fbk_var_const,)* ]
            }
            fn get_optional_feedback(&self) -> Vec<$constraint> {
                vec![ $($constraint::$fbk_opt_const,)* ]
            }

            fn get_feedback_outputs(&self) -> Vec<$constraint> {
                vec![ $($constraint::$fbk_out_const,)* ]
            }
        }

        impl $crate::CortexNode<$constraint, $data> for $lobe
            where Self: $crate::Lobe<Input=$input, Output=$output>
        {
            fn start(
                &mut self,
                this: $crate::NodeHdl,
                inputs: Vec<$crate::NodeHdl>,
                outputs: Vec<$crate::NodeHdl>
            )
                -> $crate::Result<()>
            {
                <Self as $crate::Lobe>::start(self, this, inputs, outputs)
            }

            fn update(&mut self, data: $data) -> $crate::Result<()> {
                <Self as $crate::Lobe>::update(
                    self,
                    <
                        $input as $crate::FromCortexData<$data>
                    >::from_cortex_data(data)?
                )
            }

            fn tailor_output(&mut self, output: $crate::NodeHdl)
                -> $crate::Result<$data>
            {
                Ok(
                    <
                        $output as $crate::IntoCortexData<$data>
                    >::into_cortex_data(
                        <Self as $crate::Lobe>::tailor_output(
                            self, output
                        )?
                    )?
                )
            }

            fn feedback(&mut self, data: $data) -> $crate::Result<()> {
                <Self as $crate::Lobe>::feedback(
                    self,
                    <
                        $feedback_input as $crate::FromCortexData<$data>
                    >::from_cortex_data(data)?
                )
            }

            fn tailor_feedback(&mut self, input: $crate::NodeHdl)
                -> $crate::Result<$data>
            {
                Ok(
                    <
                        $feedback_output as $crate::IntoCortexData<$data>
                    >::into_cortex_data(
                        <Self as $crate::Lobe>::tailor_feedback(
                            self, input
                        )?
                    )?
                )
            }

            fn stop(&mut self) -> $crate::Result<()> {
                <Self as $crate::Lobe>::stop(self)
            }
        }
    }
}
