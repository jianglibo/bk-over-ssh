use std::env;
extern crate vcpkg;

fn main() {
    // env::set_var("VCPKG_ROOT", "/home/jianglibo/ws/vcpkg");
    vcpkg::find_package("sqlite3").unwrap();
// output goes target/debug/build/<pkg>/output
                for (key, value) in env::vars_os() {
                println!("{:?}: {:?}", key, value);
            }

}