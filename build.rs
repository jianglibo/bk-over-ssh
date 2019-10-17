use std::env;
extern crate vcpkg;

fn main() {
    // env::set_var("VCPKG_ROOT", "/home/jianglibo/ws/vcpkg");
    // C:\\Users\\Administrator\\vcpkg\\installed\\x64-windows\\include
    // env::set_var("VCPKG_ROOT", "C:\\Users\\Administrator\\vcpkg");
    // env::set_var("TARGET", "x86_64-pc-windows-msvc");
    // vcpkg.exe install sqlite3:
    let mut env_lines = String::new();
    let env = env::vars().for_each(|(k,v)| {
        env_lines.push_str(&format!("{}={}\n", k, v));
    });
    // panic!("{}", env_lines);
    vcpkg::find_package("sqlite3").expect("sqlite3 should be found.");
    vcpkg::find_package("libssh2").expect("libssh2 should be found.");
// output goes target/debug/build/<pkg>/output
                for (key, value) in env::vars_os() {
                eprintln!("{:?}: {:?}", key, value);
            }

}

        // CARGO_CFG_TARGET_FEATURE is set in response to
        // RUSTFLAGS=-Ctarget-feature=+crt-static. It would
        //  be nice to test that also.

// new-item -path env:RUSTFLAGS -Value "-Ctarget-feature=+crt-static"
// when above flag set, will find sqlite3:x64-windows-static.

// PS D:\ws\vcpkg-rs\vcpkg_cli> vcpkg.exe list
// sqlite3:x64-windows-static                         3.29.0-1         SQLite is a software library that implements a s...
// sqlite3:x86-windows                                3.29.0-1         SQLite is a software library that implements a s...
// sqlite3:x86-windows-static                         3.29.0-1         SQLite is a software library that implements a s...

// environment:
//   RUST: stable
//   VCPKG_PANIC: 1
//   matrix:
//     - TARGET: x86_64-pc-windows-msvc
//       RUSTFLAGS: -Ctarget-feature=+crt-static
//       VCPKG_DEFAULT_TRIPLET: x64-windows-static
//     - TARGET: x86_64-pc-windows-msvc
//       VCPKG_DEFAULT_TRIPLET: x64-windows
//       VCPKGRS_DYNAMIC: 1
//     - TARGET: i686-pc-windows-msvc
//       RUSTFLAGS: -Ctarget-feature=+crt-static
//       VCPKG_DEFAULT_TRIPLET: x86-windows-static
//     - TARGET: i686-pc-windows-msvc
//       VCPKG_DEFAULT_TRIPLET: x86-windows
//       VCPKGRS_DYNAMIC: 