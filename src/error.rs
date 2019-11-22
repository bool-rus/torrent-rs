
#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display="Acess to None Object")]
    None
}

pub trait AsResult<T:Sized> {
    fn as_result(self) -> Result<T,Error>;
}

impl<T: Sized> AsResult<T> for Option<T> {
    fn as_result(self) -> Result<T, Error> {
        match self {
            None => Err(Error::None),
            Some(obj) => Ok(obj),
        }
    }
}