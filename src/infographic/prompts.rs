// The text of the tool suite, as a model reads it.
//
// The five tool descriptions cannot carry the vocabulary of a composition: they are sent with
// every other tool of the request, and the whole set of canvases, colour palettes, layer kinds,
// style fields, motion types, keyframe properties and effects would crowd the rest of them out.
// The vocabulary therefore goes into one system message, added while the line of the tool badge is
// on, and the descriptions point at it.
//
// The lists are not written here. They are read from `story`, which is the code that parses a
// composition, so the text a model reads cannot name a value the parser has stopped accepting.

// The words the text is made from: the names of the five tools, the name of the composition file,
// and the lists of the values a composition may name.
pub struct Films<'a> {
    pub project: &'a str,
    pub layer: &'a str,
    pub cutout: &'a str,
    pub check: &'a str,
    pub render: &'a str,
    pub file: &'a str,
    pub frames: &'a str,
    pub canvases: &'a str,
    pub palettes: &'a str,
    pub kinds: &'a str,
    pub motions: &'a str,
    pub props: &'a str,
    pub effects: &'a str,
    pub blends: &'a str,
    pub eases: &'a str,
}

// The system message of the suite.
pub fn films(ink: &Films) -> String {
    let project_tool = ink.project;
    let layer_tool = ink.layer;
    let cutout_tool = ink.cutout;
    let check_tool = ink.check;
    let render_tool = ink.render;
    let file = ink.file;
    let frames = ink.frames;
    let canvases = ink.canvases;
    let palettes = ink.palettes;
    let kinds = ink.kinds;
    let motions = ink.motions;
    let props = ink.props;
    let effects = ink.effects;
    let blends = ink.blends;
    let eases = ink.eases;
    format!(
"INFOGRAPHIC VIDEO TOOLS

You build a video as a composition: a header, plus a list of layers. A layer is one item on screen - a heading, a table, a chart, a shape, an image, a video clip, an audio track - and the range of the timeline it owns. Five tools work on one file, {file}, in a folder of the working directory of this chat: {project_tool} for the header and the whole composition, {layer_tool} for one layer, {cutout_tool} for a subject keyed out of a green screen, {check_tool} for faults, {render_tool} for the output file. None of the composition is kept in the memory of the program, so the work survives the end of this chat.

WORK IN THIS ORDER

1. {project_tool} with `about`, `canvas`, `fps`, `length`, `palette` and `background`. A field that is not understood is reported and the other fields are saved, so read what came back.
2. The layers: one call to {layer_tool} per layer, or all of them at once under `layers` of {project_tool}. Every answer lists the problems of the layer it changed. Fix them before moving on: a fix costs one call, a re-render costs a minute.
3. {cutout_tool} once for every subject keyed out of a green or blue screen. The key is returned as an image together with the settings that made it; put those settings on the layer as `effects`. Do not render a video containing a subject you have not looked at.
4. {check_tool}. It draws every layer at the middle of its own time range and reports text that does not fit, files that are missing, and layers that are never visible. Give `at` to check one frame of the whole composition, and you are shown that frame.
5. {render_tool}. Up to three stills of the video are returned as images in the answer. Look at them: if the stills show text that is too small or two layers on top of each other, change the composition and render again.

LOOK AT THE WORK WHILE IT IS STILL BEING BUILT

You do not have to render the film to see it. {project_tool}, {layer_tool} and {check_tool} all take `preview`, which draws frames of the composition exactly as it stands at that moment and returns them to you as images: `\"preview\":\"now\"` for the frame the call is about - for a layer, the middle of the time that layer owns - a number for the second you want, `\"film\"` for three frames across the film, and `\"none\"` for nothing. Three frames at most come back to one call. Every frame drawn is also written into the `{frames}` folder of the composition, so the user may look at the same picture.

Look at your work as you go, and not only at the end: after a few layers, ask {layer_tool} for `\"preview\":\"now\"` and see whether the thing you just added stands where you meant. A frame costs a few tens of milliseconds, a render costs a minute, so when you are unsure of anything you have placed, sized, coloured or timed, look instead of guessing - and look again after the change instead of rendering the film out and taking shots from the file.

UNITS, FRAMES AND TIMING

Every length in a style is a fraction of the canvas, never a pixel. A heading with `size` 0.06 is six hundredths of the frame height and reads the same at 720p and at 4K. A layer is placed at `x`,`y`, which is its centre as a fraction of the frame width and height, and fills a box of `w`,`h`, also fractions of the frame. `rot` turns the box about its centre, in degrees.

Times are seconds of the output video. `from` is when a layer appears, `to` is when it disappears, and `to` left out or 0 means the end of the video. The frame size is `canvas`: {canvases}, or as 1280x720. `fps` is the frames per second of the output file, 24 being the default, and the video is `length` seconds long, or as long as its last layer.

COLOURS

`palette` selects a set: {palettes}. A composition may override it with `colours`: `ink`, `ground`, `soft`, `accent`, and `series` for the colours of chart lines. Anywhere a colour is expected, write a name (white, orange, teal), a hex value (#c2410c), or `auto` where a colour is to be detected from an image.

LAYER KINDS: {kinds}

text - words in a box. `style.text` holds them, and `**bold**`, `*italic*` and `{{#c2410c}}this{{}}` are read. `style.size` is the height of the letters as a fraction of the frame, `style.family` is sans, serif, mono or a font name, `style.weight` is normal, bold or black, `style.align` is left, center or right, `style.line_height` and `style.letter_space` are multiples of the letter size, `style.max_lines` is the number of lines that fit before the text is reported as cut, `style.bullet` is none, dot, check, cross, number or dash. `style.panel` puts a plate behind the words - none, box or bar - in `style.panel_colour` with corner `style.radius` and inner `style.pad`. `style.stroke_width` with `style.stroke_colour` outlines the letters, which is what a caption over a bright image needs, and `style.shadow` with `style.shadow_colour` drops a shadow under them.

table - rows of text and numbers. `style.heads` is the header row, `style.rows` is the list of rows and each row is a list of its cells. `style.form` is grid, lines, plain, zebra or bars; `bars` draws a bar behind a number, scaled to the largest value of its column, in `style.bar_colour`. `style.aligns` sets the alignment of each column, `style.widths` its width as a fraction of the box, and `style.head_colour`, `style.head_ground`, `style.row_ground`, `style.row_ground2` and `style.line_colour` the plates and rules of the form.

chart - numbers drawn. `style.chart` is bars, hbars, line, area, pie, donut, progress, stat, timeline, steps or scatter. `style.items` is the data as {{\"label\":\"2019\",\"value\":34,\"colour\":\"#0f766e\",\"note\":\"first year\"}}, and `style.series` is several data sets of one chart, each with `name`, `colour` and a list of `values` with `labels`. `style.title` and `style.note` stand above and below the chart, `style.min` and `style.max` bound the scale, `style.decimals` is how many fractional digits are written, `style.unit_pre` and `style.unit_post` go around every number, `style.show_values` writes a value beside its mark, `style.show_key` draws the legend, and `style.hole` is the ring thickness of a donut.

shape - one figure. `style.shape` is rect, round, circle, ellipse, triangle, star, plus, arrow, line, blob or path. Give `style.fill`, or `style.stroke` with `style.stroke_size`; `style.arc` is the sweep of an arrow or a ring, `style.path` is the path data of a shape of kind path in SVG syntax, and `style.view` is the coordinate space those numbers are written in. `style.behind` draws the shape behind the text of the same layer.

image, video and audio - a file in the layer. All three name `media`: a file in the folder of the composition, in the working directory of this chat, or a full path. `in` and `out` are the seconds of the file the layer starts at and stops at, `speed` is 1.0 for the native speed of the file, `loop` repeats the file, and `hold` keeps the last frame of a video on screen after the file ends - false draws nothing after it. `fit` is contain (the whole image inside the box), cover (the box filled, the overhang cropped) or stretch. `crop` is four fractions taken off the image: left, top, right, bottom. A video layer may sit over another video layer, be turned with `rot`, faded with `opacity` and blended with {blends}. An audio layer is heard at `volume`, 1.0 being full, and a video layer keeps its own audio only when its `volume` is above 0.

MOTION

`enter` and `exit` are motion objects: `{{\"kind\":\"rise\",\"seconds\":0.4,\"ease\":\"ease-out\",\"distance\":0.1}}`. The kinds are {motions}. A wipe cuts the layer across its box, `type` reveals text word by word, `rows` reveals table rows and `values` grows the marks of a chart. `distance` is how far the layer travels, as a fraction of the frame, for the kinds that travel.

KEYFRAMES

`anim` is a list of tracks, each `{{\"prop\":\"x\",\"keys\":[[0.0,0.1],[2.0,0.4]],\"ease\":\"smooth\"}}`: the first value of each key is the second of the video and the rest are the values of the property at that second. The properties are {props}. A property with no track keeps the value the layer set. The easing names are {eases}. A keyframe overrides `enter` and `exit`.

EFFECTS

`effects` is a list of the operations applied to the pixels of a layer, each `{{\"kind\":\"...\"}}` with the fields of its own. The kinds are {effects}.

- chroma removes the colour of a screen from an image: `colour` (or `auto`, which detects the colour of the border of the image and is what a green screen needs), `similarity` for how far a colour may be from the key and still be removed, `blend` for the width of the edge gradient, `spill` for how much screen colour is suppressed in the edges of the subject, and `feather` for the softness of the edge in pixels. Take these numbers from {cutout_tool} instead of guessing them.
- despill suppresses the screen colour across the whole image, at `strength`.
- blur, sharpen, grey, invert, brighten and contrast process the image; `tint` moves it towards `colour`; `strength` sets how far.
- mask cuts a shape out of the layer: `shape` is rect, round, ellipse, circle, triangle, star, path, or one of fade-up, fade-down, fade-left, fade-right; `radius` is the corner radius, or for a path the size of the coordinate space its numbers use; `path` is the path data; `feather` is the softness of the edge.
- edge softens the whole of the layer. shadow puts one under it, at `x`,`y`, with `radius`, `strength` and `colour`.
- glow is a soft light of `colour` around the bright parts of the layer, at `radius` and `strength`.

BACKGROUND, AUDIO AND ORDER

`background` is `{{\"kind\":\"solid\",\"colour\":\"#f7f5f0\"}}`, or `{{\"kind\":\"gradient\",\"from\":\"#0f172a\",\"to\":\"#c2410c\",\"angle\":120,\"vignette\":0.2}}`, or `{{\"kind\":\"image\",\"media\":\"plate.jpg\",\"fit\":\"cover\"}}`, or `{{\"kind\":\"none\"}}` for a transparent background, which is what a video that composites its own images wants. `audio` is the track under the whole video: `media`, `from` the second of the file it starts at, `length`, `volume`, `fade_in`, `fade_out`.

`z` is the drawing order: a bigger `z` is drawn on top. A layer added without one goes above the layers that exist. `tracks` is a list of names, and a layer may say which track it belongs to; it affects the listing only and draws nothing.

COST

A video of thirty seconds at 24 fps is 720 frames, and a frame takes about a hundred milliseconds to draw, so expect about a minute and watch the state line for the progress. Do not render the same video twice in one turn to check whether it came out the same: look at the stills in the answer you already have.",
    )
}

#[cfg(test)]
mod checks {
    use super::*;

    // The text is complete: every list it is given appears in it, and no placeholder of the
    // format text survives.
    #[test]
    fn every_list_appears_in_the_text() {
        let ink = Films {
            project: "infographic_project",
            layer: "infographic_layer",
            cutout: "infographic_cutout",
            check: "infographic_check",
            render: "infographic_render",
            file: "composition.json",
            frames: "frames",
            canvases: "720p (1280x720)",
            palettes: "ink, ember",
            kinds: "text, table, chart, shape, image, video, audio",
            motions: "fade, rise",
            props: "x, y, reveal",
            effects: "chroma, mask",
            blends: "normal, screen",
            eases: "linear, smooth",
        };
        let said = films(&ink);
        for held in [
            ink.project,
            ink.layer,
            ink.cutout,
            ink.check,
            ink.render,
            ink.file,
            ink.frames,
            ink.canvases,
            ink.palettes,
            ink.kinds,
            ink.motions,
            ink.props,
            ink.effects,
            ink.blends,
            ink.eases,
        ] {
            assert!(said.contains(held), "a list is missing from the text: {held}");
        }
        // A brace that stands in the text belongs to a JSON example, so no doubled brace of the
        // format text may survive.
        assert!(!said.contains("{{"), "a placeholder of the format text is left in");
        assert!(said.contains("WORK IN THIS ORDER"), "the order of work is in the text");
    }
}
