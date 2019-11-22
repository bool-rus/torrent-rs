extern crate bytes;
extern crate failure;
extern crate futures_util;
extern crate bip_metainfo;
extern crate async_std;

#[macro_use] extern crate failure_derive;
#[macro_use] extern crate async_trait;

pub use bytes::Bytes;

pub mod message;
mod faces;
mod implements;
mod error;
mod parser;
mod peer;
mod io;

pub use faces::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }

    use failure::{Error, Fail};

    struct Impl;
    impl Impl {
        fn impl_foo<T: Fail>(obj: T) -> impl Foo {
            obj
        }
    }

    trait Foo : Fail {
        fn bar(&self) -> String;
    }

    impl<T: Fail> Foo for T {
        fn bar(&self) -> String {
            "bgg".to_string()
        }
    }
}
