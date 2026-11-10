use cargo_metadata::MetadataCommand;

fn generate_dependencies() {
    let metadata = MetadataCommand::new().exec().unwrap();

    let package: &cargo_metadata::Package = metadata
        .packages
        .iter()
        .find(|p| p.name == env!("CARGO_PKG_NAME"))
        .unwrap();

    let node = metadata
        .resolve
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|n| n.id == package.id)
        .unwrap();

    let dependencies: String = node
        .deps
        .iter()
        .filter(|dep| dep.name.starts_with("grai_"))
        .filter_map(|dep| {
            metadata
                .packages
                .iter()
                .find(|p| p.id == dep.pkg)
                .map(|p| format!("{}:{}", p.name.clone(), p.version.to_string()))
        })
        .collect::<Vec<_>>()
        .join(";");

    println!("cargo:rustc-env=BUILD__DEPENDENCIES={}", dependencies);
}

fn main() {
    #[allow(non_snake_case)]
    let BUILD__CONFIGURATION: String = {
        match std::env::var("BUILD__CONFIGURATION") {
            Ok(build_configuration) => build_configuration,
            Err(e) => {
                let build_configuration: String = "snapshot".to_string();
                eprintln!(
                    "Use default value for BUILD__CONFIGURATION: '{}' due to {}",
                    &build_configuration, e
                );
                build_configuration
            }
        }
    };

    println!(
        "cargo:rustc-env=BUILD__CONFIGURATION={}",
        BUILD__CONFIGURATION
    );

    generate_dependencies();
}
