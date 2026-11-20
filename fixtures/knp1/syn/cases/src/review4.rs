// Regression cases for the fourth review of PR #215: banned names in macro transcribers whatever
// follows them, names followed by a metavariable, `cfg_attr` items, attribute and derive macros.
macro_rules! go {
    ($b:tt $a:tt) => {
        env $b $a //~ KNP1-ENV=env
    };
}

macro_rules! many {
    ($a:tt) => {
        include_str $a; include $a; include_bytes $a; option_env $a; thread_local $a; asm $a; //~ KNP1-INCLUDE=include_str KNP1-INCLUDE=include#2 KNP1-INCLUDE=include_bytes KNP1-ENV=option_env KNP1-THREAD=thread_local KNP1-UNSAFE=asm
        1.0f64.sin $a; cos $a; exp $a; ln $a; log $a; tan $a; gamma $a; //~ KNP1-LIBM=sin KNP1-LIBM=cos KNP1-LIBM=exp KNP1-LIBM=ln KNP1-LIBM=log KNP1-LIBM=tan KNP1-LIBM=gamma
        x.max $a; min $a; clamp $a; //~ KNP1-MINMAX=max KNP1-MINMAX=min KNP1-MINMAX=clamp
        env $($r)* //~ KNP1-ENV=env
    };
}

pub fn calls() {
    go!(! ("SECRET"));
    other!(max $y); //~ KNP1-MINMAX=max
}

#[cfg_attr(all(), my_attr(env))] //~ KNP1-PARSE=my_attr KNP1-ENV=env
pub fn cfg_attr_item() {}

#[cfg_attr(test, derive(Debug), inline)]
pub fn cfg_attr_builtin() {}

#[derive(Clone, serde::Serialize, Wrapper)] //~ KNP1-PARSE=Serialize KNP1-PARSE=Wrapper
pub struct Derived;
