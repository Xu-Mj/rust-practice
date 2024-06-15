use std::process::Command;

trait BuilderExtend {
    fn with_sqlx_type(self, path: &[&str]) -> Self;
    fn with_serde(self, path: &[&str]) -> Self;
}

impl BuilderExtend for tonic_build::Builder {
    fn with_serde(self, path: &[&str]) -> Self {
        path.iter().fold(self, |acc, path| {
            acc.type_attribute(path, "#[derive(serde::Serialize, serde::Deserialize)]")
        })
    }

    fn with_sqlx_type(self, path: &[&str]) -> Self {
        path.iter().fold(self, |acc, path| {
            acc.type_attribute(path, "#[derive(sqlx::Type)]")
        })
    }
}

fn main() {
    tonic_build::configure()
        .out_dir("src/pb")
        .with_serde(&["Msg"])
        .compile(&["protos/messages.proto"], &["protos"])
        .unwrap();

    // execute cargo fmt command
    Command::new("cargo").arg("fmt").output().unwrap();

    println!("cargo: rerun-if-changed=abi/protos/messages.proto");
}
