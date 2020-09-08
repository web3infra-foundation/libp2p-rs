pub mod connection;
pub mod error;
mod frame;
mod pause;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
