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
}
