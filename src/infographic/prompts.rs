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

Build videos from a header and timed layers. The five tools share {file} in this chat's working directory: {project_tool} creates or reads a composition, {layer_tool} edits layers, {cutout_tool} keys a screen subject, {check_tool} finds faults, and {render_tool} writes the output. Compositions persist after the chat.

WORK IN THIS ORDER

1. Set the header with {project_tool}: about, canvas, fps, length, palette and background. If a field is rejected, other fields are still saved; read the result.
2. Add layers with {layer_tool}, or replace the full list with layers in {project_tool}. Edits preserve omitted fields. Fix reported issues before rendering.
3. For each green/blue-screen subject, call {cutout_tool}, inspect the preview, then copy its settings to that layer's chroma effects. Check every key before rendering.
4. Run {check_tool} and fix overflow, missing media, hidden layers and empty frames. Use at to inspect a specific frame.
5. Render with {render_tool} and inspect the returned stills. Change and rerender only when they reveal an issue.

PREVIEW WHILE BUILDING

The preview field on {project_tool}, {layer_tool} and {check_tool} draws up to three images without rendering: now means the current frame (or a layer's midpoint), a number selects a second, film samples three points, and none disables previews. Frames are also saved in the composition's {frames} folder. Inspect as you build; preview is faster than rendering.

UNITS, FRAMES AND TIMING

Sizes and positions are fractions of the canvas, not pixels: x,y locate a layer's centre, w,h set its box, and rot is in degrees. style.size is a fraction of frame height.

Times are seconds. from starts a layer; to ends it, with missing or zero to meaning the video end. canvas is {canvases} or width x height (for example 1280x720); fps defaults to 24. length defaults to the end of the last layer.

COLOURS

palette selects from {palettes}. Override colours with ink, ground, soft, accent, and chart series. Use a colour name, hex value, or auto when the colour should be detected from an image.

LAYER KINDS: {kinds}

Put these type-specific properties inside layer.style.

text - `style.text` supports **bold**, *italic* and `{{#c2410c}}coloured{{}}` spans. `size` is a fraction of frame height; `family` is sans, serif, mono or a font name; `weight` is normal, bold or black; `align` is left, center or right. `line_height` and `letter_space` are letter-size multiples; `max_lines` reports overflow; `bullet` is none, dot, check, cross, number or dash. `panel` is none, box or bar, with `panel_colour`, `radius` and `pad`. `stroke_width`/`stroke_colour` outline text; `shadow`/`shadow_colour` add a shadow.

table - `heads` is the header row; `rows` is a list of cell lists. `form` is grid, lines, plain, zebra or bars; bars scale to each column's maximum and use `bar_colour`. `aligns` and `widths` set per-column alignment and box-width fractions. `head_colour`, `head_ground`, `row_ground`, `row_ground2` and `line_colour` style cells and rules.

chart - `chart` is bars, hbars, line, area, pie, donut, progress, stat, timeline, steps or scatter. `items` holds label/value entries with optional colour/note; `series` holds named, coloured datasets with values and labels. `title`/`note` label the chart; `min`/`max` bound the scale; `decimals`, `unit_pre` and `unit_post` format values; `show_values` labels marks, `show_key` draws a legend, and `hole` sets donut-ring thickness.

shape - `shape` is rect, round, circle, ellipse, triangle, star, plus, arrow, line, blob or path. Set `fill` or `stroke`/`stroke_size`; `arc` controls an arrow or ring sweep; `path` is SVG path data and `view` its coordinate space. `behind` draws it under same-layer text.

image, video and audio - `media` names a file in the composition folder, chat working directory or an absolute path. `in`/`out` select source seconds; `speed` defaults to 1; `loop` repeats; `hold` retains a video's last frame. `fit` is contain, cover or stretch; `crop` gives left/top/right/bottom fractions. Videos support `rot`, `opacity` and blend modes {blends}; audio uses `volume`, and a video's own audio is enabled above 0.

MOTION

`enter`/`exit` are motion objects such as `{{\"kind\":\"rise\",\"seconds\":0.4,\"ease\":\"ease-out\",\"distance\":0.1}}`; kinds: {motions}. Wipe reveals across the box; type reveals words, rows reveals table rows, and values grows chart marks. `distance` is travel as a frame fraction.

KEYFRAMES

`anim` tracks use objects such as `{{\"prop\":\"x\",\"keys\":[[0.0,0.1],[2.0,0.4]],\"ease\":\"smooth\"}}`; each key starts with video seconds followed by property values. Properties: {props}; easing: {eases}. Untracked properties keep their layer values. Keyframes override `enter`/`exit`.

EFFECTS

`effects` is a list of pixel operations, each with its own fields. Kinds: {effects}.

- chroma removes a screen colour: use `colour` or `auto`, plus `similarity`, `blend`, `spill`, `feather`, `choke` and `despill`. Get key settings from {cutout_tool}, not guesses.
- despill suppresses screen colour across the image with `strength`.
- blur, sharpen, grey, invert, brighten, contrast and tint process pixels; tint uses `colour`, and `strength` sets effect amount.
- mask clips to rect, round, ellipse, circle, triangle, star, path or fade-up/down/left/right. Set `radius`, SVG `path`/coordinate `view`, and `feather` as needed.
- edge softens the layer. shadow uses `x`, `y`, `radius`, `strength` and `colour`.
- glow adds light around bright pixels using `colour`, `radius` and `strength`.

BACKGROUND, AUDIO AND ORDER

`background` accepts solid (`colour`), gradient (`from`, `to`, `angle`, `vignette`), image (`media`, `fit`) or none (transparent). `audio` sets the global track with `media`, `from`, `length`, `volume`, `fade_in` and `fade_out`.

Higher `z` draws on top; new layers default above existing ones. `tracks` labels layers for listing only and does not affect drawing.

COST

A 30-second video may take about a minute to render; monitor progress. Inspect returned stills, fix visible issues, and do not rerender unchanged work.",
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
