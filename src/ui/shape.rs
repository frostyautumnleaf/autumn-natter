// The places and the sizes of the main window. More than one drawing step needs
// them, and the motion of the row of previews works from them too, so they stand
// here instead of inside one long drawing function.
//
// The chat card and the chat box below use the same x and the same width, so the
// two always stand in line.

// The shape of one window of a given size.
pub struct Shape {
    // Size the window has now.
    pub width: f32,
    pub height: f32,
    // True when the window is narrow, so the top bar gives way in good order.
    pub tight: bool,
    // Height of the top bar.
    pub bar: f32,
    // The status line, the room between, and the chat box together.
    pub dock_height: f32,
    // Place and width of the middle card.
    pub card_x: f32,
    pub card_width: f32,
    // The room between the top bar and the dock, where the cards stand.
    pub stage_top: f32,
    pub stage_height: f32,
    // Place of the dock. At home it stands in the middle of the window.
    pub dock_y: f32,
    // Widest the two boxes of the top bar may grow.
    pub model_cap: f32,
    pub repo_cap: f32,
    // Shape of the row of small previews.
    pub prev_width: f32,
    // The room from one resting place of the row to the next.
    pub prev_step: f32,
    pub prev_height: f32,
    pub prev_y: f32,
}

impl Shape {
    // The shape of a window of this size. True says that no chat is open.
    pub fn new(width: f32, height: f32, home: bool) -> Shape {
        let inset = 18.0;
        let bar = 52.0;
        let status = 30.0;
        let box_height = 118.0;
        let tight = width < 860.0;
        let dock_height = status + 8.0 + box_height;
        // Only the chat in use is drawn, so the card has the room that the
        // neighbours of the carousel used to have.
        let card_x = inset;
        let card_width = (width - 2.0 * inset).max(280.0);
        let stage_top = bar + 12.0;
        // At home the mark, the greeting and the box stand as one group, and the
        // group sits in the middle of the room below the top bar. With a chat
        // open the box stands at the bottom of the window.
        let dock_y = if home {
            bar + 178.0 + (height - bar - 178.0 - dock_height) / 2.0
        } else {
            height - dock_height - 14.0
        };
        let stage_height = (dock_y - stage_top - 12.0).max(0.0);
        // Room that the top bar spends on things other than the two selector
        // boxes, so the two boxes know how much is left for them.
        let bar_fixed = 24.0
            + 50.0
            + if tight { 84.0 } else { 118.0 }
            + if tight { 0.0 } else { 168.0 }
            + if tight { 26.0 } else { 140.0 };
        let chip_room = (width - bar_fixed).max(180.0);
        let prev_width = ((width - 40.0) / 4.2).clamp(180.0, 300.0);
        let prev_gap = 18.0;
        let prev_height = (prev_width * 1.24).min(stage_height - 78.0).max(120.0);
        Shape {
            width,
            height,
            tight,
            bar,
            dock_height,
            card_x,
            card_width,
            stage_top,
            stage_height,
            dock_y,
            model_cap: chip_room.mul_add(0.56, 0.0).min(if tight { 190.0 } else { 320.0 }),
            repo_cap: chip_room.mul_add(0.44, 0.0).min(if tight { 140.0 } else { 230.0 }),
            prev_width,
            prev_step: prev_width + prev_gap,
            prev_height,
            prev_y: ((stage_height - prev_height) / 2.0 - 14.0).max(8.0),
        }
    }

    // Where preview number `place` stands inside the stage. The row is pushed
    // away from its resting place by `spin`.
    pub fn prev_x(&self, place: f32, chat_index: usize, spin: f32) -> f32 {
        (self.width - self.prev_width) / 2.0
            + (place - chat_index as f32) * self.prev_step
            - spin
    }

    // The resting places of the row lie a whole number of steps apart.
    pub fn align(&self, value: f32) -> f32 {
        let steps = (value / self.prev_step).round();
        steps * self.prev_step
    }

}
