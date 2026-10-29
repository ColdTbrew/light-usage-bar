#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn to_top_left(self, primary_top: f64) -> Self {
        Self {
            y: primary_top - self.y - self.height,
            ..self
        }
    }

    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

pub struct Screen {
    pub id: u32,
    pub frame: Rect,
    pub visible: Rect,
}

pub fn place(anchor: Rect, screens: &[Screen]) -> Option<(u32, Rect)> {
    let screen = screens
        .iter()
        .find(|s| {
            s.frame
                .contains(anchor.x + anchor.width / 2., anchor.y + anchor.height / 2.)
        })
        .or_else(|| screens.first())?;
    let visible = screen.visible;
    let width = 460_f64.min(visible.width);
    let height = 620_f64.min(visible.height);
    Some((
        screen.id,
        Rect {
            x: (anchor.x + anchor.width - width)
                .clamp(visible.x, visible.x + visible.width - width)
                - screen.frame.x,
            y: (anchor.y + anchor.height).clamp(visible.y, visible.y + visible.height - height)
                - screen.frame.y,
            width,
            height,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn secondary_screen_uses_local_coordinates_and_its_display_id() {
        let screens = [
            Screen {
                id: 1,
                frame: rect(0., 0., 1920., 1080.),
                visible: rect(0., 24., 1920., 1000.),
            },
            Screen {
                id: 2,
                frame: rect(-1280., -200., 1280., 1024.),
                visible: rect(-1200., -176., 1200., 1000.),
            },
        ];
        let (id, bounds) = place(rect(-1250., -200., 100., 24.), &screens).unwrap();
        assert_eq!(id, 2);
        assert_eq!(bounds, rect(80., 24., 460., 620.));
        let (id, bounds) = place(rect(-100., -200., 100., 24.), &screens).unwrap();
        assert_eq!(id, 2);
        assert_eq!(bounds, rect(820., 24., 460., 620.));
    }

    #[test]
    fn above_primary_screen_and_small_visible_area_fit_without_covering_dock() {
        let screen = Screen {
            id: 3,
            frame: rect(0., 1080., 800., 600.).to_top_left(1080.),
            visible: rect(60., 1140., 740., 516.).to_top_left(1080.),
        };
        let anchor = rect(700., -600., 100., 24.);
        let (id, bounds) = place(anchor, &[screen]).unwrap();
        assert_eq!(id, 3);
        assert_eq!(bounds, rect(340., 24., 460., 516.));
    }

    #[test]
    fn narrow_screen_resizes_width_and_missing_screen_falls_back() {
        let screens = [Screen {
            id: 7,
            frame: rect(0., 0., 400., 600.),
            visible: rect(0., 20., 400., 580.),
        }];
        assert_eq!(
            place(rect(900., 0., 50., 20.), &screens),
            Some((7, rect(0., 20., 400., 580.)))
        );
        assert_eq!(place(rect(0., 0., 1., 1.), &[]), None);
    }
}
