use embed_manifest::{embed_manifest, new_manifest};
use std::process::{Command};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        //Statically link vcruntime140.dll
        static_vcruntime::metabuild();

        //Embed a Windows manifest
        embed_manifest(new_manifest("Piton")).expect("unable to embed manifest file");
    }


    if std::env::var_os("CARGO_FEATURE_TESTAPP").is_some() {
        println!("cargo:rerun-if-changed=test");
        Command::new("dotnet")
            .arg("build")
            .arg("test/Test.csproj")
            .status()
            .expect("Failed to build test C#");
    }
    
    println!("cargo:rerun-if-changed=build.rs")
}
