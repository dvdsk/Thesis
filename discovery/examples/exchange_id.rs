use std::env;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let id: u32 = env::args()
        .skip(1)
        .next()
        .expect("have to pass at least one arg")
        .parse()
        .expect("pass id as u32");

    println!("id: {}", id);
    discovery::maintain().await;
}
