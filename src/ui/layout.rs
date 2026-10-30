//! Fits strip items into the free taskbar space (spec §6.7).

use crate::model::Kind;

/// Item widths measured by the renderer, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemMetrics {
    pub full_w: i32,
    pub compact_w: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Label row above value and bar.
    Full,
    /// Value and bar only.
    Compact,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub mode: Mode,
    /// Indices into the input slice, in display order.
    pub visible: Vec<usize>,
    /// Input index of the last hardware item, when limit items follow it.
    pub separator_after: Option<usize>,
    /// Items were dropped; an ellipsis is drawn after the last visible one.
    pub overflow: bool,
    pub width: i32,
}

fn measure(
    items: &[(Kind, ItemMetrics)],
    count: usize,
    mode: Mode,
    gap: i32,
    sep_w: i32,
) -> (i32, Option<usize>) {
    let shown = &items[..count];
    let mut width: i32 = shown
        .iter()
        .map(|(_, m)| match mode {
            Mode::Full => m.full_w,
            Mode::Compact => m.compact_w,
        })
        .sum();
    width += gap * (count as i32 - 1).max(0);
    let last_hw = shown.iter().rposition(|(k, _)| *k == Kind::Hardware);
    let has_limits = shown.iter().any(|(k, _)| *k != Kind::Hardware);
    let separator_after = last_hw.filter(|_| has_limits);
    if separator_after.is_some() {
        width += sep_w + gap;
    }
    (width, separator_after)
}

/// Full with everything, else Compact with everything, else Compact dropping items from
/// the end with an ellipsis. Returns width 0 when nothing fits.
pub fn layout(
    items: &[(Kind, ItemMetrics)],
    available: i32,
    gap: i32,
    sep_w: i32,
    ellipsis_w: i32,
) -> Layout {
    let n = items.len();
    for mode in [Mode::Full, Mode::Compact] {
        let (width, separator_after) = measure(items, n, mode, gap, sep_w);
        if width <= available {
            return Layout {
                mode,
                visible: (0..n).collect(),
                separator_after,
                overflow: false,
                width,
            };
        }
    }
    for count in (1..n).rev() {
        let (width, separator_after) = measure(items, count, Mode::Compact, gap, sep_w);
        let width = width + gap + ellipsis_w;
        if width <= available {
            return Layout {
                mode: Mode::Compact,
                visible: (0..count).collect(),
                separator_after,
                overflow: true,
                width,
            };
        }
    }
    Layout {
        mode: Mode::Compact,
        visible: Vec::new(),
        separator_after: None,
        overflow: n > 0,
        width: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Kind;

    const GAP: i32 = 10;
    const SEP: i32 = 1;
    const ELL: i32 = 12;

    fn items() -> Vec<(Kind, ItemMetrics)> {
        let hw = ItemMetrics {
            full_w: 40,
            compact_w: 30,
        };
        let cl = ItemMetrics {
            full_w: 50,
            compact_w: 30,
        };
        vec![
            (Kind::Hardware, hw),
            (Kind::Hardware, hw),
            (Kind::Hardware, hw),
            (Kind::Claude, cl),
            (Kind::Claude, cl),
        ]
    }

    #[test]
    fn fits_full() {
        let l = layout(&items(), 300, GAP, SEP, ELL);
        assert_eq!(l.mode, Mode::Full);
        assert_eq!(l.visible, vec![0, 1, 2, 3, 4]);
        assert_eq!(l.separator_after, Some(2));
        assert!(!l.overflow);
        assert_eq!(l.width, 271);
    }

    #[test]
    fn falls_back_to_compact() {
        let l = layout(&items(), 250, GAP, SEP, ELL);
        assert_eq!(l.mode, Mode::Compact);
        assert_eq!(l.visible.len(), 5);
        assert!(!l.overflow);
        assert_eq!(l.width, 201);
    }

    #[test]
    fn drops_from_end_with_ellipsis() {
        let l = layout(&items(), 150, GAP, SEP, ELL);
        assert_eq!(l.mode, Mode::Compact);
        assert_eq!(l.visible, vec![0, 1, 2]);
        assert!(l.overflow);
        assert_eq!(l.separator_after, None);
        assert_eq!(l.width, 132);
        assert!(l.width <= 150);
    }

    #[test]
    fn separator_only_between_groups() {
        let hw_only: Vec<_> = items().into_iter().take(3).collect();
        assert_eq!(layout(&hw_only, 500, GAP, SEP, ELL).separator_after, None);
        let limits_only: Vec<_> = items().into_iter().skip(3).collect();
        assert_eq!(
            layout(&limits_only, 500, GAP, SEP, ELL).separator_after,
            None
        );
    }

    #[test]
    fn empty_items_zero_width() {
        let l = layout(&[], 500, GAP, SEP, ELL);
        assert!(l.visible.is_empty());
        assert_eq!(l.width, 0);
        assert!(!l.overflow);
    }

    #[test]
    fn nothing_fits_hides_strip() {
        let l = layout(&items(), 5, GAP, SEP, ELL);
        assert!(l.visible.is_empty());
        assert_eq!(l.width, 0);
    }
}
