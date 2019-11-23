mod arg_parse;

#[async_std::main]
async fn main() {
    let args = std::env::args();

    let user_input = arg_parse::capture_input(args);

    dbg!(user_input);
}
