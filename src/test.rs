
#[cfg(test)]
use std::{path::Path, process::Command as ExecProcess};
#[cfg(test)]
use which::which;

#[test]
fn create_test_pools() {
    fn exec_bash(bash_command: &Path) {
        let _process_output = ExecProcess::new(bash_command)
            .arg("-c")
            .arg("../tests/test_helper.sh")
            .arg("create_pools")
            .output()
            .unwrap();
    }

    if let Ok(bash_command) = which("bash") {
        exec_bash(&bash_command)
    } else {
        panic!()
    }
}

#[test]
fn destroy_test_pools() {
    fn exec_bash(bash_command: &Path) {
        let _process_output = ExecProcess::new(bash_command)
            .arg("-c")
            .arg("../tests/test_helper.sh")
            .arg("destroy_pools")
            .output()
            .unwrap();
    }

    if let Ok(bash_command) = which("bash") {
        exec_bash(&bash_command)
    } else {
        panic!()
    }
}
