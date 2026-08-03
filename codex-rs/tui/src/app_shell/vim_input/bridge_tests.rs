use super::script;

#[test]
fn generated_vim_bridge() {
    insta::assert_snapshot!("vim_bridge", script());
}
