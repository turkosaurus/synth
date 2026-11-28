use super::commas;

#[test]
fn groups_digits_in_threes() {
    assert_eq!(commas(0), "0");
    assert_eq!(commas(480), "480");
    assert_eq!(commas(5760), "5,760");
    assert_eq!(commas(12000), "12,000");
    assert_eq!(commas(1234567), "1,234,567");
}
