// Regression cases for the third review of PR #215: macro-defining macros and escaped
// metavariables, macro names passed to macros whatever follows them, attribute arguments, and
// aliased integer types in `<…>::pow`.
macro_rules! outer {
    ($d:tt) => {
        macro_rules! inner { //~ KNP1-PARSE=macro_rules
            ($d m:ident $d a:tt) => { $d m! $d a }; //~ KNP1-PARSE=$#3
        }
    };
}

pub fn calls() {
    inner!(env ("SECRET")); //~ KNP1-ENV=env
    inner!(option_env ("X")); //~ KNP1-ENV=option_env
    inner!(include_str ("data.txt")); //~ KNP1-IO-FS=include_str
    inner!(include ("gen.in")); //~ KNP1-INCLUDE=include
    inner!(include_bytes ("table.bin")); //~ KNP1-IO-FS=include_bytes
    inner!(thread_local { static K: u8 = 0; }); //~ KNP1-THREAD=thread_local
}

#[my_attr(env, sin)] //~ KNP1-PARSE=my_attr KNP1-ENV=env KNP1-LIBM=sin
pub fn attr_args() {}

#[allow(unknown_lints, max)]
#[derive(Debug)]
pub struct Plain;

#[allow(non_camel_case_types)]
type i32 = f64;

pub fn pow_alias() -> i32 {
    <i32>::pow(2.0, 3) + i32::pow(2.0, 3) //~ KNP1-POW=pow#1 KNP1-POW=pow#2
}
