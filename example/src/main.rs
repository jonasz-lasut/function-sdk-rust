//! The composition function's CLI entrypoint.

use clap::Parser;
use crossplane_function_sdk::{Args, logging, serve};

mod function;

#[tokio::main]
async fn main() -> Result<(), crossplane_function_sdk::Error> {
    let args = Args::parse();
    logging::configure(args.debug);
    serve(function::Function, &args).await
}
