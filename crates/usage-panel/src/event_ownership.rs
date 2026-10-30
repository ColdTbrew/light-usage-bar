use usage_display::ScreenAnchor;

#[derive(Clone, Copy)]
pub struct PanelOwnership {
    pub anchor: ScreenAnchor,
    pub host_pid: i32,
    pub helper_pid: i32,
}

impl PanelOwnership {
    pub fn owns_activation(self, pid: i32) -> bool {
        pid == self.host_pid || pid == self.helper_pid
    }

    pub fn owns_click(self, primary: bool, control_pressed: bool, x: f64, y: f64) -> bool {
        primary
            && !control_pressed
            && x >= self.anchor.x
            && x < self.anchor.x + self.anchor.width
            && y >= self.anchor.y
            && y < self.anchor.y + self.anchor.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ownership() -> PanelOwnership {
        PanelOwnership {
            anchor: ScreenAnchor {
                x: -500.,
                y: -1080.,
                width: 24.,
                height: 24.,
            },
            host_pid: 100,
            helper_pid: 101,
        }
    }

    #[test]
    fn primary_anchor_click_stays_owned_on_displays_above_and_left() {
        let owner = ownership();
        assert!(owner.owns_click(true, false, -500., -1080.));
        assert!(owner.owns_click(true, false, -488., -1068.));
        for (x, y) in [
            (-501., -1068.),
            (-476., -1068.),
            (-488., -1081.),
            (-488., -1056.),
        ] {
            assert!(!owner.owns_click(true, false, x, y));
        }
        assert!(!owner.owns_click(false, false, -488., -1068.));
        assert!(!owner.owns_click(true, true, -488., -1068.));
    }

    #[test]
    fn activation_belongs_only_to_host_and_helper() {
        let owner = ownership();
        assert!(owner.owns_activation(100));
        assert!(owner.owns_activation(101));
        assert!(!owner.owns_activation(102));
    }
}
