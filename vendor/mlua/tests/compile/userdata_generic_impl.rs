#[derive(mlua::UserData)]
struct Foo {
    x: u32,
}

#[mlua::userdata_impl]
impl<T> Foo<T> {
    #[lua(infallible)]
    fn get(&self) -> u32 {
        self.x
    }
}

fn main() {}
