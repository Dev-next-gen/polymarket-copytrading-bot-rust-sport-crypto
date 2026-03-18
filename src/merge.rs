#[derive(Debug, Clone, PartialEq)]
pub struct MergeResult {
    pub complete_sets: f64,
    pub remaining_up: f64,
    pub remaining_down: f64,
}


pub fn merge_up_down_amounts(up_amount: f64, down_amount: f64) -> MergeResult {
    let complete_sets = up_amount.min(down_amount);
    let remaining_up = (up_amount - complete_sets).max(0.0);
    let remaining_down = (down_amount - complete_sets).max(0.0);
    MergeResult {
        complete_sets,
        remaining_up,
        remaining_down,
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_up_down_amounts, MergeResult};

    #[test]
    fn equal_amounts() {
        let r = merge_up_down_amounts(5.0, 5.0);
        assert_eq!(r, MergeResult { complete_sets: 5.0, remaining_up: 0.0, remaining_down: 0.0 });
    }

    #[test]
    fn more_up_than_down() {
        let r = merge_up_down_amounts(5.0, 3.0);
        assert_eq!(r, MergeResult { complete_sets: 3.0, remaining_up: 2.0, remaining_down: 0.0 });
    }

    #[test]
    fn more_down_than_up() {
        let r = merge_up_down_amounts(2.0, 7.0);
        assert_eq!(r, MergeResult { complete_sets: 2.0, remaining_up: 0.0, remaining_down: 5.0 });
    }

    #[test]
    fn zero_up() {
        let r = merge_up_down_amounts(0.0, 5.0);
        assert_eq!(r, MergeResult { complete_sets: 0.0, remaining_up: 0.0, remaining_down: 5.0 });
    }

    #[test]
    fn zero_down() {
        let r = merge_up_down_amounts(5.0, 0.0);
        assert_eq!(r, MergeResult { complete_sets: 0.0, remaining_up: 5.0, remaining_down: 0.0 });
    }

    #[test]
    fn both_zero() {
        let r = merge_up_down_amounts(0.0, 0.0);
        assert_eq!(r, MergeResult { complete_sets: 0.0, remaining_up: 0.0, remaining_down: 0.0 });
    }

    #[test]
    fn fractional() {
        let r = merge_up_down_amounts(2.5, 1.5);
        assert_eq!(r, MergeResult { complete_sets: 1.5, remaining_up: 1.0, remaining_down: 0.0 });
    }
}
