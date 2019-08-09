use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize, Builder)]
#[builder(setter(into))]
pub struct FileItem<'a, 'b, 'c> {
    pub remote_path: &'a str,
    #[builder(default = "None")]
    pub local_path: Option<&'b str>,
    pub sha1: &'c str,
    pub len: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_file_item() {
        let s = 
r#"
{"remote_path": "a",
"local_path": "b",
"sha1": "cc",
"len": 55
}
"#;
    let fi: FileItem = serde_json::from_str(&s).expect("load develope_data should success.");
    assert_eq!(fi.remote_path, "a");
    }

    #[test]
    fn new_file_item() {
        let fi = FileItemBuilder::default().remote_path("a").sha1("kk").len(55_u64).build().expect("should create new file item.");
        let fi_str = serde_json::to_string(&fi).expect("should serialize.");
        println!("{:?}", fi_str);
        let fi1 = serde_json::from_str::<FileItem>(&fi_str).expect("should deserialize.");
        
        assert!(fi1.local_path.is_none());
    }

}