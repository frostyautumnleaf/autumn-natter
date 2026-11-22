// Program settings and the place where Autumn Natter keeps its data.
//
// The settings file is a small JSON file. It lives in the program data directory. The directory
// name is always autumn_natter.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::proglog;

// Name of the data directory. Do not change it without a move step.
pub const APP_DIR_NAME: &str = "autumn_natter";

// Default port for the remote (browser) mode.
pub const DEFAULT_REMOTE_PORT: u16 = 8723;

fn default_remote_port() -> u16 {
    DEFAULT_REMOTE_PORT
}

// A switch of the Chatting section that is on unless the user turns it off.
fn on_by_default() -> bool {
    true
}

// A switch of the Chatting section that stays off until the user turns it on.
fn off_by_default() -> bool {
    false
}

// The embedding model of a set of settings that names none: the model of the program, at its
// biggest quantization, with its vision projector. See `crate::embed`.
fn default_embedding_model() -> String {
    String::from(crate::embed::DEFAULT_MODEL)
}

fn default_embedding_quant() -> String {
    String::from(crate::embed::DEFAULT_QUANT)
}

fn default_embedding_mmproj() -> String {
    String::from(crate::embed::DEFAULT_MMPROJ)
}

// How the server of the embeddings is run: whether the card is used at all, how many tokens are
// counted at once, how many threads of the computer the counting may use, and the length of one
// ask in tokens. The counting of the sources is a background job, so it is kept on the computer's
// own cores until the user says otherwise. See `crate::embed::Run`.
fn default_embedding_cpu_only() -> bool {
    crate::embed::RUN_CPU_ONLY
}

fn default_embedding_batch() -> u32 {
    crate::embed::RUN_BATCH
}

fn default_embedding_threads() -> u32 {
    crate::embed::RUN_THREADS
}

fn default_embedding_ctx() -> u32 {
    crate::embed::RUN_CTX
}

// ------------------------------------------------------------ compaction
//
// A chat goes on for ever once the size of the context is rebuilt on the way. The compaction
// reports how that is done: either nothing at all, or a handoff summary. The summary is written
// by the model itself when the conversation nears the end of its space, and the text before
// it are then left out of what goes back to the model. Only the first message of the user and
// the newest summary go on, beside the text that came after the summary.

// No compaction. The chat is sent whole until the space is removed.
pub const COMPACTION_OFF: &str = "off";
// The handoff summary. This is what a new set of settings is on.
pub const COMPACTION_HANDOFF: &str = "handoff";

// What falls in the background of a chat. Nothing falls unless the user requests it, so this is
// what a new set of settings is on.
pub const BACKGROUND_NONE: &str = "none";
// Autumn leaves fall behind the chat, drawn from the leaf of the program icon, and only while a
// model is answering. The effect is drawn by the page; the engine only carries the word.
pub const BACKGROUND_LEAVES: &str = "leaves";
// Pixelated rain falls behind the chat whenever the setting remains, whether a model is
// answering or not. Where the rain meets the chat composer box or a block of the chat, it gives
// a pixelated splash, unless the splash switch below is turned off. The effect is drawn by the
// page.
pub const BACKGROUND_RAIN: &str = "rain";

// The summarisation is set going by a share of the size of the context.
pub const COMPACT_BY_PERCENT: &str = "percent";
// The summarisation is set going by the least space that must be left over.
pub const COMPACT_BY_TOKENS: &str = "tokens";

// The most characters the handoff prompt may hold. The prompt is sent with all of the
// conversation, so a prompt of a novel would cost space of its own.
pub const GREATEST_HANDOFF_PROMPT: usize = 4096;

// The most characters the system prompt of the user may hold. The text of the pre-written
// prompts are not counted here: they are in the program and are of a size the program answers
// for. The whole block of text before the conversation, the pre-written prompts and the text of
// the user together, is kept under GREATEST_SYSTEM_PROMPT by the settings and by the chat.
pub const GREATEST_SYSTEM_PROMPT: usize = 8000;

// The prompt that is given to the model when it is requested the handoff summary. The user may
// write another one in the settings.
pub const DEFAULT_HANDOFF_PROMPT: &str = "Write a detailed but concise handoff summary for AI consumption (no styling) that can be given to an AI to continue working from the current state of the session. Include the context of the session that an AI needs to carry on. Say what has been done so far and what needs to be done next. The AI will only receive the first initial prompt that the user sent, along with this handoff summary. Name the file paths that matter, one short line each, with what each file is used for. Keep the summary to about three hundred words: it must come to an end, for a summary that runs on for ever leaves the space it was asked to free.";

// The second request a handoff summary. The first is given all the space that is above after
// the text of the conversation, and a model with a great deal to say can fill that space to
// the end and stop in the middle of a sentence. The second ask carries the same conversation
// and a prompt that leans hard the other way, so the text comes to an end inside the space that
// is left.
pub const DEFAULT_HANDOFF_PROMPT_SHORT: &str = "Write a very short handoff summary for AI consumption (no styling). The context window of the model is nearly full, so the whole summary must end within a few hundred words. Keep only what an AI needs to continue the work from this point: what is done, what is half done, and what to do next. Name the files that matter, one short line each. Do not repeat the conversation, do not review finished work, and do not add detail that is not needed to continue.";

// The time one call of a tool is given, of a new set of settings: ten minutes.
fn default_tool_seconds() -> u32 {
    600
}

// The share of the size of the context at which the handoff summary is requested.
fn default_compact_percent() -> u32 {
    85
}

// The least space that must be left over, in tokens. This is the number the second way of
// setting the summarisation going works from.
fn default_compact_reserve() -> u32 {
    10240
}

// The handoff prompt of a new set of settings.
fn default_handoff_prompt() -> String {
    String::from(DEFAULT_HANDOFF_PROMPT)
}

// The handoff prompt of the second ask, of a new set of settings.
fn default_handoff_prompt_short() -> String {
    String::from(DEFAULT_HANDOFF_PROMPT_SHORT)
}

// The share of the cards the model may work in. A number that is no share at all -- nothing, or
// more than a hundred -- is the share a new set of settings is on, so a settings file written
// when this field held mebibytes returns as a share instead of as a surprise.
pub fn sanitize_vram_budget(share: f64) -> f64 {
    if !(1.0..=100.0).contains(&share) {
        return 90.0;
    }
    share
}

// The space for steps of one answer, as the engine wants it. The setting is -1 for no
// limit, so the answer is None. A number below -1 is read as no limit too, for a ceiling of
// minus five means nothing. The engine crate keeps this line, so the setting of the window and
// the ceiling of the loop cannot drift apart.
pub fn engine_steps(setting: i64) -> Option<usize> {
    if setting < 0 {
        return None;
    }
    Some(setting as usize)
}

// Model settings. One entry per model file. The model path is the key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSettings {
    pub ctx_size: u32,
    // Index inside the KV cache type list in llama.rs.
    pub kv_index: usize,
    // Index inside the speculative decoding list in llama.rs.
    pub spec_index: usize,
    pub draft_model: String,
    pub draft_tokens: u32,
    // The three numbers ngram-mod is given, for the two choices of the settings panel that
    // request it. 0 keeps the number llama.cpp is on, so a set of settings written before these
    // fields existed guesses as it did: 24 for the length of the look-up, 64 for the most text
    // guessed at once and 48 for the least.
    pub ngram_match: i64,
    pub ngram_max: i64,
    pub ngram_min: i64,
    pub parallel: u32,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub tensor_parallel: bool,
    pub vision: bool,
    // Sampling of one answer. The defaults are the values of this program, and they are sent to
    // the server on every start. The values are f64, so the settings file and the window show
    // 0.7, not the full form of an f32.
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub min_p: f64,
    // 1.0 turns the penalty off.
    pub repeat_penalty: f64,
    // 0.0 turns the penalty off.
    pub presence_penalty: f64,
    // The most tokens one answer may give. -1 leaves no limit.
    pub max_tokens: i64,
    // The seed of the random draw. -1 picks a new one on every start.
    pub seed: i64,
    // The most context checkpoints the server keeps per slot. 0 turns them off.
    pub ctx_checkpoints: i64,
    // The ceiling of the conversation memory in mebibytes. 0 turns the cache off, and -1 lifts
    // the limit.
    pub cache_ram: i64,
    // The server keeps the work of the prompt and reuses it on the next turn.
    pub cache_prompt: bool,
    // The space one image may take, for a model that can look at images. 0 keeps the value
    // inside the model file.
    pub image_min_tokens: i64,
    pub image_max_tokens: i64,
    // Stop the shift of the context when the conversation overflows the space. Good to use
    // when coding, where a shift cuts the text the model works on.
    pub no_context_shift: bool,
    // The projector that reads the images works on the card, by default. With the switch on, it
    // works on the CPU.
    pub mmproj_cpu: bool,
    // -1 means no limit. 0 stops thinking at once.
    pub reasoning_budget: i64,
    pub reasoning_message: String,
    // Custom jinja chat template. Empty uses the model's own embedded template.
    #[serde(default)]
    pub chat_template: String,
    // Index inside the model load mode list in llama.rs.
    pub load_index: usize,
    pub extra_args: String,
    // Send the arguments this program builds from the fields above. With the switch off, only
    // the text that connect the program to a server go out: the model, its name, the address
    // and the port. A server of another build that knows none of the arguments of this program
    // is then run with a command line of the user's own, put into the field of the extra
    // arguments. The extra arguments are still sent last, so they win over the four words of
    // the connection, the model above all.
    pub send_default_args: bool,
    // How much of the cards the model may work in, as a share of what the cards hold together:
    // 90 means nine tenths of the cards, and the tenth is above. The fit of the model into the
    // cards is requested of llama.cpp always, so this is not a switch but a share. A number
    // outside one hundred is read as the share a new set of settings is on, which takes care of
    // a settings file written when this held mebibytes.
    pub vram_budget: f64,
    // One flag per detected device, in detection order.
    pub gpu_enabled: Vec<bool>,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            ctx_size: 8192,
            kv_index: 1,
            spec_index: 0,
            draft_model: String::new(),
            draft_tokens: 4,
            ngram_match: 0,
            ngram_max: 0,
            ngram_min: 0,
            parallel: 1,
            batch_size: 512,
            ubatch_size: 128,
            tensor_parallel: false,
            vision: false,
            temperature: 0.7,
            top_p: 0.95,
            top_k: 20,
            min_p: 0.0,
            repeat_penalty: 1.0,
            presence_penalty: 0.0,
            max_tokens: -1,
            seed: -1,
            ctx_checkpoints: 32,
            cache_ram: 8192,
            cache_prompt: true,
            image_min_tokens: 0,
            image_max_tokens: 0,
            no_context_shift: false,
            mmproj_cpu: false,
            reasoning_budget: -1,
            reasoning_message: String::new(),
            chat_template: String::new(),
            // mmap keeps the model in the file cache.
            load_index: 1,
            extra_args: String::new(),
            // The arguments of this program go out unless the user reports otherwise.
            send_default_args: true,
            vram_budget: 90.0,
            gpu_enabled: Vec::new(),
        }
    }
}

impl ModelSettings {
    // Merge a GPU list of a different length into the stored one.
    pub fn resize_gpus(&mut self, count: usize) {
        self.gpu_enabled.resize(count, true);
    }
}

// All program settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    // Directory that holds the llama-server binary.
    pub llama_dir: String,
    // Directory that holds the model files.
    pub models_dir: String,
    // The embedding model that counts the words of the sources of retrieval, and its
    // quantization. The words are the ones the catalog in `src/embed.rs` knows; a word that
    // names nothing is read as the model of the program at the biggest quantization. Nothing is
    // downloaded by these settings alone: the model arrives when the Install button is pressed.
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    #[serde(default = "default_embedding_quant")]
    pub embedding_quant: String,
    // The vision projector of the embedding model, or `none` when the contents of a picture are
    // not wanted. A projector that is in the folder anyway is used, whatever is written here.
    #[serde(default = "default_embedding_mmproj")]
    pub embedding_mmproj: String,
    // How the server of the counting is run. On by default, the toggle keeps the counting on the
    // cores of the computer, so a search of the sources never takes the card the answers come
    // from. The three numbers are the batch of tokens counted at once, the threads the counting
    // may use - zero leaves the choice to llama.cpp - and the length of one ask in tokens. Each
    // is taken back into its bounds when the file is read; see `crate::embed::Run`.
    #[serde(default = "default_embedding_cpu_only")]
    pub embedding_cpu_only: bool,
    #[serde(default = "default_embedding_batch")]
    pub embedding_batch: u32,
    #[serde(default = "default_embedding_threads")]
    pub embedding_threads: u32,
    #[serde(default = "default_embedding_ctx")]
    pub embedding_ctx: u32,
    // Directory that holds the repositories. Empty means the default place.
    pub repositories_dir: String,
    // Another place for the settings, the conversations, the logs and the records of this
    // program. Empty means the usual place: the `autumn_natter` folder of the program data of
    // the operating system, or the folder `--data-dir` named.
    //
    // The text cannot be in the settings file alone, for the program must know where to look
    // before it can read a setting. The place is therefore kept in a marker file that always is
    // in the usual place: see `mark_file`. The folder must remain already - a word that names
    // no folder is no place - and nothing is moved: the program reads from the new place when
    // it starts again, and until then from the place it is reading from.
    #[serde(default)]
    pub data_dir: String,
    // Another place for the contexts the model has processed: the read conversation of each
    // chat, which llama-server writes and reads back. Empty means the `contexts` folder inside
    // the data folder. The folder is created when it is requested. Nothing is moved here
    // either: a conversation that is in the old place is read no more, and the chat is then
    // read through again from its first word.
    #[serde(default)]
    pub contexts_dir: String,
    // The theme the pages are drawn in: `light`, `dark`, `silver`, `corporate` or `neon`. An
    // empty word markers a settings file written before there were three themes, and the switch
    // below answers for it.
    #[serde(default)]
    pub theme: String,
    // True when the theme is drawn in its pixel-art form: the colours of the theme above, with
    // all of the interface rebuilt out of squares - no rounded corner, no blur and no gradient.
    // It belongs to the theme rather than to a chat, so every theme has a standard and a
    // pixel-art form.
    #[serde(default)]
    pub theme_pixel: bool,
    // True when this file was written by a program that knows the four themes. A file without
    // the word holds `corporate` for the ladder of grey, which now answers to `silver`: the
    // near-black theme took the older name. The marker is placed on when the settings are read,
    // and the file is written again, so the two are never mixed up again after that.
    #[serde(default)]
    pub skins_v2: bool,
    // The switch of the two themes that was before the third. It is kept in step with the theme
    // above, so that a settings file of this program can still be read by a program written
    // when there were two.
    pub dark_mode: bool,
    #[serde(default = "default_remote_port")]
    pub remote_port: u16,
    pub upnp_enabled: bool,
    // Model the user selected last.
    pub model_id: String,
    // Per model settings. The key is the full model path.
    pub model_settings: BTreeMap<String, ModelSettings>,
    // The presets of the settings of a model: named forms of all of those settings, which the
    // Model tab of the settings panel keeps for the user. A preset is tied to no model, for it
    // is one way of setting a model up rather than the setting of one model -- a chat of long
    // answers on the big card, say, or a chat that never shifts the context. The text is all of
    // the settings of a model, and are in the same format, so a preset and a model are
    // distinguished by the name alone.
    #[serde(default)]
    pub model_presets: BTreeMap<String, ModelSettings>,
    // Chatting. How the chat of the window shows the parts of an answer. Fold a thinking panel
    // up while the thinking of the model arrives.
    #[serde(default = "on_by_default")]
    pub chat_collapse_thinking: bool,
    // Let a thinking panel grow with its text instead of stopping at a height the user moves
    // in. Off, every thinking panel keeps the fixed height it always had.
    #[serde(default = "on_by_default")]
    pub chat_grow_thinking: bool,
    // Let a tool block that the model calls while thinking are inside the thinking panel of
    // that turn, instead of under it.
    #[serde(default = "on_by_default")]
    pub chat_tools_in_thinking: bool,
    // Fold a tool panel up while its command runs.
    #[serde(default = "on_by_default")]
    pub chat_collapse_tools: bool,
    // Tell the model the day and the clock time of this computer once, in a message of the role
    // `system` at the start of the conversation. The line is not shown in the chat.
    #[serde(default = "on_by_default")]
    pub chat_date_new_chat: bool,
    // Give the text of a block of code the colours of their type.
    #[serde(default = "on_by_default")]
    pub chat_highlight: bool,
    // Let the markers of boldness mean boldness: a word held between two markers is written in
    // the heavier form of the letters. Turned off, the markers are read as part of the text of
    // the answer, as they were before.
    #[serde(default = "on_by_default")]
    pub chat_bold_markdown: bool,
    // Let the colours the model requests remain as colours: a run of text held between
    // `<green>` and `</green>`, or between the markers of the three other colours, is shown in
    // that colour. Turned off, the text is shown with no colour of their own. The markers are
    // taken out of the conversation that goes back to the model only while this is on, for the
    // page cannot paint what it no longer reads.
    #[serde(default = "on_by_default")]
    pub chat_colour_text: bool,
    // Read the chat as one piece of writing. The panels of the thoughts and of the calls then
    // are on the chat itself: no ground of their own, no line about them, and no rule of colour
    // down the left edge of a thought. The marker that opens a panel is beside its heading, for
    // with nothing drawn about a panel the far right of its header is far from the text.
    #[serde(default)]
    pub chat_seamless: bool,
    // How the work of the model is shown. "plain" is the chat as it has always was: the
    // thoughts in their panels and every call in a block of its own. "corporate" shows none of
    // that: one block means an answer, its header saying what the model is at, and the model
    // writes the summary of its work into that block with a tool of its own that no switch of
    // the user can remove. The other ways of laying the chat out have nothing to say when this
    // one remains, so the settings of them go grey over it.
    #[serde(default = "plain_display")]
    pub chat_display: String,
    // True once the screen the program puts up when it is started for the very first time has
    // been removed through. A set of settings written by the program is with false, so the
    // screen remains; a set of settings that came from a file of a program that had no such
    // screen is with true, for the user of it has chosen their way long before.
    #[serde(default = "welcomed_before")]
    pub welcomed: bool,
    // How many times an answer is asked over when the model came to its own end without a word,
    // a call or an error. A model that thought a while and then stopped has stated nothing of
    // the work it was given, and the same ask a second time very nearly always brings the text
    // back. 0 leaves a silence alone.
    #[serde(default = "silence_retries")]
    pub chat_silent_retries: i64,
    // Read the chapters of a chat out of the headings of the answers, and show them beside it.
    // The text that tell the model of the rule go with the switch, so a chat that shows no
    // chapter line never pays for them.
    #[serde(default = "on_by_default")]
    pub chat_chapters: bool,
    // Show the messages of the user on a line of their own, hard against the right edge of the
    // chat, as if the chat were bordered by them: one short dash for one message, and a little
    // of the message on the card of the dash. The line is there so a user of a long chat can go
    // back to an item they asked without scrolling all of the conversation over.
    #[serde(default = "on_by_default")]
    pub chat_userline: bool,
    // Draw the background of the chat as a faint grid of hairlines instead of as a solid colour.
    // The grid is the plain form of the background - the pixel-art appearance has its own check
    // of squares - and it is drawn in the colours of the theme, so it is legible in every skin
    // and never fights the rain, the leaves or the digits the cursor uncovers. Now and then one
    // line of the grid goes into a slow wavy oscillation and comes back to straight again, at a
    // place and a time drawn at random. All of it is on the screen: nothing of a chat is stored
    // the different for the grid, and a chat that is asked to stay still shows the grid with no
    // movement in it at all.
    #[serde(default = "on_by_default")]
    pub chat_grid: bool,
    // What falls behind the text of a chat: "none", or "leaves" for autumn leaves drawn from
    // the leaf of the program icon, which fall only while a model is answering. The leaves are
    // drawn by the page, and they never catch the pointer.
    #[serde(default = "default_background")]
    pub chat_background: String,
    // Whether the pixelated rain splashes where it meets the chat composer box and the blocks
    // of the chat. It is under the rain: with no rain falling there is nothing to splash. The
    // splashes are placed anew each time, so the same spot is never splashed twice over.
    #[serde(default = "on_by_default")]
    pub chat_rain_splash: bool,
    // Whether the cursor uncovers a field of random digits in the background of the chat. The
    // digits stand in rows and columns, in the colour of the leaf mark, and the soft round aura
    // about the pointer reveals them for as long as the pointer rests there: the edge of the
    // aura is feathered, so the digits come in with no line drawn anywhere, and it breathes
    // while the pointer stands still. All of it is on the screen, and nothing is stored.
    // The field is off until the user asks for it.
    #[serde(default = "off_by_default")]
    pub chat_cursor_digits: bool,
    // Carry a message you sent into the chat with a little movement: the text leave the box, go
    // over the chat and unfold into the message itself. The movement is only on the screen.
    // Nothing about the sending waits for it, and a chat informed to stay still shows the
    // message where it always was, at once.
    #[serde(default = "on_by_default")]
    pub chat_send_anim: bool,
    // Keep the conversation of a chat that the model has worked its way through in a file of
    // the program, so the chat does not have to be read through again when it returns: after
    // another chat took the model, or after the program was shut down and started again. What
    // is in the file is what the model understood of the conversation, and not the conversation
    // itself -- that is where it always was -- so the file is only ever a saving of the
    // reading, and it may be deleted whenever the user likes.
    #[serde(default = "on_by_default")]
    pub chat_context_files: bool,
    // Write the saved context files in the compressed form described in `src/contexts.rs`: the
    // attention cache of the model is held as 16-bit numbers instead of plain text, which costs
    // a little of the reading for a great deal less space on the disk. Off by default, so
    // the files are in the form they have always was in until a user requests the other one.
    // Turning the switch off again leaves the files that were already written compressed --
    // they are still read -- and new ones go back to the plain form.
    #[serde(default)]
    pub chat_context_compression: bool,
    // How many terminal commands one answer may run. -1 leaves no limit, so a model may run a
    // command as often as it wants inside one answer.
    #[serde(default = "no_limit")]
    pub chat_max_tool_steps: i64,
    // How long one call of a tool is given to return within, in seconds. A call that is still
    // running when its time is removed is stopped, and the model is informed that it ran out of
    // time; a command that is stopped leaves behind everything it had printed. Nothing at all
    // gives a call no time to return within, which is the same as not having the feature at
    // all.
    #[serde(default = "default_tool_seconds")]
    pub chat_tool_timeout: u32,
    // How many round trips one answer may take in all: a round trip is one turn of the model,
    // and one more follows every command it requests. -1 leaves no limit, so an answer ends
    // only when the model stops by itself.
    #[serde(default = "no_limit")]
    pub chat_max_answer_steps: i64,
    // A paste of more than this many characters into the chat composer becomes a file
    // attachment instead of text in the box. 100 is the least, so a paste of a word or two
    // always stays a paste.
    #[serde(default = "default_pasted_text_char_limit")]
    pub pasted_text_char_limit: u32,
    // How wide the chat may be, in pixels. The width the chat always had is the least it can be
    // set to. The page keeps the chat within 80 per cent of the width of the window, and within
    // the space that is above after the line of the chapters, the line of the summarisations
    // and the arrows of the chat have taken their space.
    #[serde(default = "default_chat_width")]
    pub chat_width: u32,
    // How long the model waits for an answer to a question it put to the user. When the time is
    // removed the user is asked no more, the model is informed that the user did not answer,
    // and it is left to go on as well as it can.
    #[serde(default = "default_ask_seconds")]
    pub ask_seconds: u32,
    // The same for a question the model flagged as one it cannot go on without. Nothing at all,
    // which is the default, waits until the user answers: a model that was set to ask only when
    // it must should not have its question removed for a user who stepped away from the
    // computer.
    #[serde(default)]
    pub ask_critical_seconds: u32,
    // Compaction. Either "off", which leaves the chat alone until the space is removed, or
    // "handoff", which asks the model for a handoff summary before the space is removed. A new
    // set of settings is on the handoff summary.
    #[serde(default = "default_compaction")]
    pub chat_compaction: String,
    // Which way the summarisation is set going: "percent" for a share of the size of the
    // context, "tokens" for the least space that must be left over.
    #[serde(default = "default_compact_by")]
    pub chat_compact_by: String,
    // The share of the size of the context at which the summary is requested.
    #[serde(default = "default_compact_percent")]
    pub chat_compact_percent: u32,
    // The least space that must be left over, in tokens. When the space left is this small or
    // smaller, the summary is requested.
    #[serde(default = "default_compact_reserve")]
    pub chat_compact_reserve: u32,
    // The prompt that is sent to the model when it is requested the handoff summary. No longer
    // than GREATEST_HANDOFF_PROMPT characters.
    #[serde(default = "default_handoff_prompt")]
    pub chat_compact_prompt: String,
    // The prompt of the second ask: the one that is created when the first ran to the end of
    // the space. It leans towards a short summary, so the text come to an end. No longer
    // than GREATEST_HANDOFF_PROMPT characters.
    #[serde(default = "default_handoff_prompt_short")]
    pub chat_compact_prompt_short: String,
    // The text the user writes for the model to build on: the system prompt of the chat. Empty
    // gives no prompt of the user, and the chat then reads only the text of the program and the
    // flagged pre-written prompts. No longer than GREATEST_SYSTEM_PROMPT characters.
    #[serde(default)]
    pub chat_system_prompt: String,
    // The pre-written prompts the user put a marker on, by their ids. They are in front of the
    // text of the user, in the order of the table. A set of settings written before this was
    // reads with no marker at all.
    #[serde(default)]
    pub chat_system_prompts_on: Vec<String>,
    // The vertical timeline of the summarisations, at the right of the chat.
    #[serde(default = "on_by_default")]
    pub chat_timeline: bool,    // The servers of the user's own, spoken to over MCP. They stand here and
    // not in a directory of their own, for the settings panel writes the whole list again
    // whenever one of them moves. A set of settings written before this was reads with no
    // server at all.
    #[serde(default)]
    pub mcp_servers: Vec<crate::mcp::Server>,
}

// The compaction of a new set of settings.
fn default_compaction() -> String {
    String::from(COMPACTION_HANDOFF)
}

// No background of its own.
fn default_background() -> String {
    String::from(BACKGROUND_NONE)
}

// Which way a new set of settings sets the summarisation going.
fn default_compact_by() -> String {
    String::from(COMPACT_BY_PERCENT)
}

// The compaction of a word the window sent. A word that is neither of the two gives the handoff
// summary, which is what a new set of settings is on.
pub fn sanitize_compaction(value: &str) -> String {
    match value.trim() {
        COMPACTION_OFF => String::from(COMPACTION_OFF),
        _ => String::from(COMPACTION_HANDOFF),
    }
}

// The background of a chat, from the word the window sent. Only the names the page can draw are
// kept: a word from an older window, or one the page cannot draw, leaves the chat on the
// background it always had.
pub fn sanitize_background(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BACKGROUND_LEAVES => String::from(BACKGROUND_LEAVES),
        BACKGROUND_RAIN => String::from(BACKGROUND_RAIN),
        _ => String::from(BACKGROUND_NONE),
    }
}

// The system prompt of the user as the settings keep it. The text is cut to
// GREATEST_SYSTEM_PROMPT characters, and the space at either end is taken off: a prompt of
// nothing but spaces is no prompt, and the chat then reads as if the box was empty.
pub fn sanitize_system_prompt(value: &str) -> String {
    let words = value.trim();
    if words.chars().count() <= GREATEST_SYSTEM_PROMPT {
        return String::from(words);
    }
    words.chars().take(GREATEST_SYSTEM_PROMPT).collect()
}

// The markers on the pre-written prompts as the settings keep them. Only ids of prompts the
// program knows are kept, each once, and in the order of the table and not the order the window
// sent: the text go in front of the conversation in one order whatever the page sent.
pub fn sanitize_prompt_ids(values: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for p in crate::prompts::lines() {
        let id = p["id"].as_str().unwrap_or("");
        if values.iter().any(|v| v == id) {
            kept.push(String::from(id));
        }
    }
    kept
}

// The way of setting the summarisation going. A word that is neither of the two gives the share
// of the space, which is what a new set of settings is on.
pub fn sanitize_compact_by(value: &str) -> String {
    match value.trim() {
        COMPACT_BY_TOKENS => String::from(COMPACT_BY_TOKENS),
        _ => String::from(COMPACT_BY_PERCENT),
    }
}

// The share of the size of the context. One per cent is the least, so a setting of zero does
// not request a summary on every message; a hundred per cent requests one when the space is
// full.
pub fn sanitize_compact_percent(value: i64) -> u32 {
    value.clamp(1, 100) as u32
}

// The least space that must be left over. One token is the least, and a number over a million
// is a mistake rather than a setting.
pub fn sanitize_compact_reserve(value: i64) -> u32 {
    value.clamp(1, 1_000_000) as u32
}

// The handoff prompt, kept inside the ceiling of its length. A field left empty gives the
// prompt of the program again, for a chat without a prompt would ask the model for nothing at
// all. The greatest length of the name of a preset, in characters. The name is read from a
// field of the settings panel and shown again in the menu of the presets, where a name longer
// than a line of the menu reads worse than the short one it was cut to.
pub const GREATEST_PRESET_NAME: usize = 60;

// The five themes the pages can be drawn in. `light` is the pale ground the program is cut for,
// `dark` the same colours on a nearly black background, `silver` grey and white and the shades
// between them alone, `corporate` a nearly black background drawn in the same neutral ladder -
// the colours of the program are put away in those two as they are not elsewhere - and `neon` a
// night of blue-black with the hairlines lit. All of them has a second form drawn out of
// squares, was on by `Config::theme_pixel`.
pub const THEME_LIGHT: &str = "light";
pub const THEME_DARK: &str = "dark";
pub const THEME_SILVER: &str = "silver";
pub const THEME_CORPORATE: &str = "corporate";
pub const THEME_NEON: &str = "neon";

// The text of the five themes, in the order the settings panel and the marker of the browser
// page step through them.
pub const THEMES: [&str; 5] = [
    THEME_LIGHT,
    THEME_DARK,
    THEME_SILVER,
    THEME_CORPORATE,
    THEME_NEON,
];

// The word of a theme. A word that is none of the five answers as the pale theme, which is the
// one a new set of settings is on. An empty word is the marker of a settings file written
// before there were four themes, and the switch of the two that such a file holds answers for
// it.
pub fn theme_word(value: &str, dark: bool) -> String {
    match value {
        THEME_DARK => THEME_DARK,
        THEME_SILVER => THEME_SILVER,
        THEME_CORPORATE => THEME_CORPORATE,
        THEME_NEON => THEME_NEON,
        "" if dark => THEME_DARK,
        _ => THEME_LIGHT,
    }
    .to_string()
}

// The name a preset is kept under: the text the user wrote, with the space off the ends and the
// space inside made even, and cut to the length the menu holds. A name of nothing at all
// returns as nothing, which is how the caller learns that there was no name there to keep.
pub fn sanitize_preset_name(given: &str) -> String {
    let words = given.split_whitespace().collect::<Vec<_>>().join(" ");
    words.chars().take(GREATEST_PRESET_NAME).collect()
}

pub fn sanitize_handoff_prompt(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::from(DEFAULT_HANDOFF_PROMPT);
    }
    if text.chars().count() <= GREATEST_HANDOFF_PROMPT {
        return text.to_string();
    }
    text.chars().take(GREATEST_HANDOFF_PROMPT).collect()
}

// The prompt of the second ask, kept inside the same ceiling. A field left empty gives the
// short prompt of the program again.
pub fn sanitize_handoff_prompt_short(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::from(DEFAULT_HANDOFF_PROMPT_SHORT);
    }
    if text.chars().count() <= GREATEST_HANDOFF_PROMPT {
        return text.to_string();
    }
    text.chars().take(GREATEST_HANDOFF_PROMPT).collect()
}

// Whether the conversation of a chat has reached the point the settings set for the handoff
// summary. `used` is the space the conversation takes and `total` the space the server
// gives. The rule is here and nowhere else, so the field of the window and the check of the
// worker cannot drift apart.
pub fn compaction_wanted(cfg: &Config, used: u64, total: u64) -> bool {
    // Nothing is known of the space, so nothing can be stated about running out.
    if total == 0 {
        return false;
    }
    used >= compaction_room(cfg, total)
}

// Where the limit set for the handoff summary remains, in the text of the conversation: the
// least space that `compaction_wanted` answers as past the limit. It is requested by an item
// that is paid for out of the space the summarisation makes -- the report of a call that
// was stopped where the model wrote it -- which has to be measured against the limit and not
// against all of the window, for a conversation that remains past the limit is stopped at the
// first word of the answer. With the summarisation switched off, or with nothing known of the
// directory, no limit is set, and the greatest number is given: all of the space is then free.
pub fn compaction_room(cfg: &Config, total: u64) -> u64 {
    if sanitize_compaction(&cfg.chat_compaction) != COMPACTION_HANDOFF || total == 0 {
        return u64::MAX;
    }
    if sanitize_compact_by(&cfg.chat_compact_by) == COMPACT_BY_TOKENS {
        let reserve = cfg.chat_compact_reserve.max(1) as u64;
        return total.saturating_sub(reserve);
    }
    let percent = cfg.chat_compact_percent.clamp(1, 100) as u64;
    // The least number that is past the share, so the rule answers the same way as the
    // multiplication it is beside.
    (total * percent).div_ceil(100)
}

// Where the summarisation of the handoff begins, in the text of the conversation, for the line
// of it on the context meter. Nothing returns when the summarisation is switched off, or when
// nothing is known of the space, or when the place it begins at is beyond all of the
// space: there is then nothing to marker.
pub fn compaction_mark(cfg: &Config, total: u64) -> Option<u64> {
    let room = compaction_room(cfg, total);
    if room == u64::MAX || room == 0 || room >= total {
        return None;
    }
    Some(room)
}

// The least a paste limit may be set to. Smaller numbers are lifted to it.
pub const LEAST_PASTE_LIMIT: u32 = 100;

// The most a paste limit may be set to, so one paste cannot request a file of more than a
// million characters.
pub const GREATEST_PASTE_LIMIT: u32 = 1_000_000;

// The place a paste of text turns into a file, in characters.
fn default_pasted_text_char_limit() -> u32 {
    6000
}

// The short wait of a question the model put to the user. Thirty seconds is long enough to read
// the question, weigh it and answer it, and short enough that a user who has removed away is
// not waited for while the model sits on its question.
fn default_ask_seconds() -> u32 {
    30
}

// The longest wait either type of question may be set to. A day of waiting is already all of a
// working day, and a wait longer than that is an error in the setting rather than a wish of the
// model.
pub const GREATEST_ASK_SECONDS: u32 = 24 * 60 * 60;

// Keep the paste limit inside the two bounds above.
pub fn sanitize_pasted_text_limit(value: u32) -> u32 {
    value.clamp(LEAST_PASTE_LIMIT, GREATEST_PASTE_LIMIT)
}

// The width the chat always had, and the least it can be set to. A chat no narrower than this
// is as readable as the chat always was.
pub const LEAST_CHAT_WIDTH: u32 = 780;

// The most the field of the window may hold, in pixels. The page keeps the chat within 80 per
// cent of the window as well, so a number above the width of the window is answered with the
// space the window has.
pub const GREATEST_CHAT_WIDTH: u32 = 4000;

// The width of the chat of the program, which is the least of it too.
fn default_chat_width() -> u32 {
    LEAST_CHAT_WIDTH
}

// Keep the width of the chat inside the two bounds above.
pub fn sanitize_chat_width(value: u32) -> u32 {
    value.clamp(LEAST_CHAT_WIDTH, GREATEST_CHAT_WIDTH)
}

// A number of steps with no ceiling. The Chatting settings hold -1 for it, and llama.cpp uses
// the same number for "as much as the model can think of".
fn no_limit() -> i64 {
    -1
}

// How many times an answer is asked over when the model came to its own end without a word, a
// call or an error in its own.
fn silence_retries() -> i64 {
    2
}

// The chat is shown as it has always been shown: the thoughts in their panels, and every call
// the model made in a block of its own.
pub const PLAIN_DISPLAY: &str = "plain";

// The chat is shown the corporate way: the thoughts and the calls are hidden whole, and one
// block means an answer, its header saying what the model is at and its body holding the
// summary of the work, which the model writes itself.
pub const CORPORATE_DISPLAY: &str = "corporate";

fn plain_display() -> String {
    String::from(PLAIN_DISPLAY)
}

// The settings of a program that has been started before this screen was.
fn welcomed_before() -> bool {
    true
}

// True when the settings request the corporate way of showing the work of the model. Any other
// word, and a word that is no word at all, is the plain way.
pub fn display_is_corporate(display: &str) -> bool {
    display == CORPORATE_DISPLAY
}

impl Default for Config {
    fn default() -> Self {
        Self {
            llama_dir: default_llama_dir(),
            models_dir: paths().models.display().to_string(),
            embedding_model: default_embedding_model(),
            embedding_quant: default_embedding_quant(),
            embedding_mmproj: default_embedding_mmproj(),
            embedding_cpu_only: default_embedding_cpu_only(),
            embedding_batch: default_embedding_batch(),
            embedding_threads: default_embedding_threads(),
            embedding_ctx: default_embedding_ctx(),
            repositories_dir: String::new(),
            // Both folders answer for themselves when the settings say nothing: the usual place
            // for the files of the program, and the `contexts` folder inside it for the
            // conversations of the model.
            data_dir: String::new(),
            contexts_dir: String::new(),
            theme: String::new(),
            theme_pixel: false,
            skins_v2: true,
            dark_mode: false,
            remote_port: DEFAULT_REMOTE_PORT,
            upnp_enabled: false,
            model_id: String::new(),
            model_settings: BTreeMap::new(),
            model_presets: BTreeMap::new(),
            chat_collapse_thinking: true,
            chat_grow_thinking: true,
            chat_tools_in_thinking: true,
            chat_collapse_tools: true,
            chat_date_new_chat: true,
            chat_highlight: true,
            chat_bold_markdown: true,
            chat_colour_text: true,
            chat_seamless: false,
            chat_display: plain_display(),
            welcomed: false,
            chat_silent_retries: 2,
            chat_chapters: true,
            chat_userline: true,
            chat_grid: true,
            chat_background: String::from(BACKGROUND_NONE),
            chat_rain_splash: true,
            chat_cursor_digits: false,
            chat_send_anim: true,
            chat_context_files: true,
            chat_context_compression: false,
            chat_max_tool_steps: -1,
            chat_tool_timeout: default_tool_seconds(),
            chat_max_answer_steps: -1,
            pasted_text_char_limit: default_pasted_text_char_limit(),
            chat_width: default_chat_width(),
            ask_seconds: default_ask_seconds(),
            ask_critical_seconds: 0,
            // The handoff summary is between the first message of the user and the end of the
            // space, so a chat can go on for ever.
            chat_compaction: default_compaction(),
            chat_compact_by: default_compact_by(),
            chat_compact_percent: default_compact_percent(),
            chat_compact_reserve: default_compact_reserve(),
            chat_compact_prompt: default_handoff_prompt(),
            chat_compact_prompt_short: default_handoff_prompt_short(),
            chat_system_prompt: String::new(),
            chat_system_prompts_on: Vec::new(),
            chat_timeline: true,
            // No server of the user's own is in a new set of settings.
            mcp_servers: Vec::new(),
        }
    }
}

impl Config {
    // Settings for one model. A missing entry gives the default settings.
    pub fn model(&self, id: &str) -> ModelSettings {
        self.model_settings.get(id).cloned().unwrap_or_default()
    }
}

// Places on disk that belong to this program.
pub struct Paths {
    // autumn_natter directory.
    pub data: PathBuf,
    // settings.json inside the data directory.
    pub settings_file: PathBuf,
    // Chats that belong to no repository.
    pub chats: PathBuf,
    // The chats of the repositories. They is here and not inside a repository, for a repository
    // is the space a terminal command works in, and it must not hold the conversations of
    // the model that works in it.
    pub repo_chats: PathBuf,
    // The record of every repository: its name, when it was created and last used, whether it
    // is pinned. These is here for the same reason the conversations do, and not in the
    // directory of the repository beside the work.
    pub repo_records: PathBuf,
    // The logs of llama-server, one file for every start of it, the ten newest of them. The
    // Logs tab of the settings panel reads them.
    pub logs: PathBuf,
    // The contexts the model has processed, one file for every chat that had the keeping of
    // them switched on. They is here, and not in the directory of a repository, for a model
    // that works in a repository must not read or write over what another chat left of itself.
    // llama-server is given this folder when it starts, and it writes the files into it by
    // name.
    pub contexts: PathBuf,
    // Default repositories directory.
    pub repositories: PathBuf,
    // Default models directory.
    pub models: PathBuf,
}

// Return the OS default place for program data. Linux: $XDG_CONFIG_HOME or ~/.config. Windows:
// %APPDATA%.
fn base_config_dir() -> PathBuf {
    if let Some(dir) = dirs::config_dir() {
        return dir;
    }
    // Fall back to a hidden directory in the home directory.
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config")
}

use std::sync::OnceLock;

// Set by --data-dir. The first call wins, and it wins over the marker file below: a command
// line is reported to the program that is in front of it.
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

// The place named by the marker file, read once for a run. A folder is not moved from under a
// program that is remaining: the conversations of a chat, the log of a server and the settings
// must all are in the one place for all of a run, so the folder the settings name is read from
// when the program starts again.
static DATA_MARK: OnceLock<Option<PathBuf>> = OnceLock::new();

// The file that names another place for the files of the program.
const MARK_FILE_NAME: &str = "data-dir.txt";

// The settings of the program, inside the data folder.
const SETTINGS_FILE_NAME: &str = "settings.json";

// The usual place for the files of this program.
fn default_data_dir() -> PathBuf {
    base_config_dir().join(APP_DIR_NAME)
}

// Where the marker file remains. It always is in the usual place, whatever the settings say and
// wherever `--data-dir` sent the files, for it is what the program reads to know where else to
// look - and a place that named a place in turn would be a scan with no end.
pub fn mark_file() -> PathBuf {
    default_data_dir().join(MARK_FILE_NAME)
}

// The place named by a marker file, and nothing for a marker that names no folder that remains.
// An empty file, text of another program's own, a folder on a disk that is not there today: all
// of them are answered by the usual place, which is where a program that cannot find its files
// must go.
pub fn read_mark(place: &Path) -> Option<PathBuf> {
    let said = fs::read_to_string(place).ok()?;
    let said = said.trim();
    if said.is_empty() {
        return None;
    }
    let dir = PathBuf::from(said);
    dir.is_dir().then_some(dir)
}

// Remain the marker for the folder the settings name, or take the marker away when they name
// none. The folder must remain already, so a spelling mistake costs nothing: the text of the
// old marker is then kept where they were, which is the place the program is reading from. When
// a marker is kept a folder that holds no settings of its own, the current settings are copied
// there, so a program that starts again at the new place starts with the settings it had - the
// conversations, the logs and the records are moved by the user, and not here.
pub fn point_data_dir(cfg: &Config) -> std::io::Result<()> {
    point_data_dir_at(&mark_file(), &paths().settings_file, cfg)
}

// The same, with the place of the marker file and the current settings file given: the tests
// remain a marker in a directory of their own rather than in the program data of this computer.
pub fn point_data_dir_at(
    place: &Path,
    settings: &Path,
    cfg: &Config,
) -> std::io::Result<()> {
    let said = cfg.data_dir.trim();
    if said.is_empty() {
        return match fs::remove_file(place) {
            Ok(()) => Ok(()),
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(why) => Err(why),
        };
    }
    let dir = PathBuf::from(said);
    if !dir.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not a folder", dir.display()),
        ));
    }
    if read_mark(place).as_deref() != Some(dir.as_path()) {
        if let Some(parent) = place.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(place, format!("{}\n", dir.display()))?;
        let there = dir.join(SETTINGS_FILE_NAME);
        if settings != there && !there.is_file() && settings.is_file() {
            let _ = fs::copy(settings, &there);
        }
    }
    Ok(())
}

// The place named by the marker file of the usual place, read once for a run.
fn marked_data_dir() -> Option<PathBuf> {
    DATA_MARK.get_or_init(|| read_mark(&mark_file())).clone()
}

// The place the files of this program are in: the directory of `--data-dir`, the directory of
// the marker file, or the usual place, in that order.
fn data_root() -> PathBuf {
    if let Some(dir) = DATA_DIR.get() {
        return dir.clone();
    }
    if let Some(dir) = marked_data_dir() {
        return dir;
    }
    default_data_dir()
}

// Read the paths of this program.
pub fn paths() -> Paths {
    let data = data_root();
    Paths {
        chats: data.join("chats"),
        repo_chats: data.join("repo_chats"),
        repo_records: data.join("repo_records"),
        logs: data.join("logs"),
        contexts: data.join("contexts"),
        repositories: data.join("repositories"),
        models: data.join("models"),
        settings_file: data.join(SETTINGS_FILE_NAME),
        data,
    }
}

// Point the data directory somewhere else. Only the start-up code calls this, and it calls it
// before any other code reads the paths.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

// The tests that write through the data folder take turns through this one lock, so no test
// empties the directory of another while it works.
#[cfg(test)]
pub fn test_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that failed while it held the lock leaves it poisoned. The tests share nothing
    // else, so the lock is taken anyway: every test then reports its own error instead of an
    // error in the test before it.
    TURN.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// Create the data directory and the folders inside it. The directories of the conversations and
// of the records are not made here: that text is in `chats.db` now, and the folders were all of
// the keeping before. `src/migrate.rs` takes what is in them into the database and retires the
// folders.
pub fn ensure_dirs() -> std::io::Result<()> {
    let p = paths();
    fs::create_dir_all(&p.data)?;
    fs::create_dir_all(&p.logs)?;
    fs::create_dir_all(&p.contexts)?;
    fs::create_dir_all(&p.models)?;
    Ok(())
}

// Read settings.json. A missing file gives the default settings. The file is in the directory
// of the data of this place, so a run that points the program at another folder with
// `--data-dir` reads the settings of that folder, and the directories of the model and of
// llama.cpp follow it.
pub fn load() -> Config {
    let file = paths().settings_file;
    let mut cfg: Config = match fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    };
    // A settings file written before the directory of the data could be chosen holds no word of
    // one. The marker file that sent this run to its folder is that word, so it is placed in
    // the settings: the field of the settings panel then shows where the files really remain,
    // and a writing of the settings remains the marker again in the same place instead of
    // taking it away.
    if cfg.data_dir.trim().is_empty() {
        if let Some(dir) = marked_data_dir() {
            if dir != default_data_dir() {
                cfg.data_dir = dir.display().to_string();
            }
        }
    }
    // A settings file written before there were three themes holds no word of one. The switch
    // of the two that such a file holds answers for it, so a chat that was on the dark theme
    // does not start up pale.
    cfg.theme = theme_word(&cfg.theme, cfg.dark_mode);
    // A file written before the fourth theme holds `corporate` for the ladder of grey. That
    // theme is called `silver` now, and the older name belongs to the near-black one, so the
    // word is turned over once and the file is written again with the marker of the four themes
    // on it.
    if turn_the_skin_words(&mut cfg) {
        let _ = save(&cfg);
    }
    // A settings file that names an embedding model this program does not know, or a quantization
    // that is not in the list of one, is read as the model of the program rather than as nothing
    // at all: a search of the sources would otherwise fail with no word about why.
    cfg.embedding_model = crate::embed::sanitize_model(&cfg.embedding_model);
    cfg.embedding_quant = crate::embed::sanitize_quant(&cfg.embedding_model, &cfg.embedding_quant);
    cfg.embedding_mmproj = crate::embed::sanitize_mmproj(&cfg.embedding_model, &cfg.embedding_mmproj);
    // The running of the counting is taken back into its bounds in the same breath: a file that
    // says a batch of nine million, or a thread count of four thousand, is read as the bound
    // nearest to what was written rather than as a server that will not start.
    let run = crate::embed::Run::new(
        cfg.embedding_cpu_only,
        cfg.embedding_batch,
        cfg.embedding_threads,
        cfg.embedding_ctx,
    );
    cfg.embedding_batch = run.batch;
    cfg.embedding_threads = run.threads;
    cfg.embedding_ctx = run.ctx;
    cfg
}

// Rename the theme words of a settings file written before the four themes existed. True is
// returned when the file was changed, which is when it is worth writing again. A file that
// already holds the four-theme marker is skipped, so the word `corporate` chosen after the
// change keeps its new meaning.
fn turn_the_skin_words(cfg: &mut Config) -> bool {
    if cfg.skins_v2 {
        return false;
    }
    if cfg.theme == THEME_CORPORATE {
        cfg.theme = THEME_SILVER.to_string();
    }
    cfg.skins_v2 = true;
    true
}

// Write settings.json. Parent directories are created when needed.
pub fn save(cfg: &Config) -> std::io::Result<()> {
    let p = paths();
    fs::create_dir_all(&p.data)?;
    let text = serde_json::to_string_pretty(cfg).unwrap_or_else(|_| String::from("{}"));
    // Write to a second file first, then rename. A crash cannot leave a short file behind.
    let tmp = p.data.join("settings.json.new");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &p.settings_file)?;
    // The marker for another data folder is kept in step with the settings, so the place the
    // program looks in when it starts again is the place the user requested. An error here
    // reports that the folder written in the settings is no folder: the settings are written
    // either way, and the program keeps reading from the place it was reading from. It is
    // reported in the log of the program, for the settings panel has no word of its own to say
    // it with.
    if let Err(why) = point_data_dir(cfg) {
        proglog::note(
            proglog::FOLDERS,
            &format!("the data folder of the settings is not was: {}", why),
        );
    }
    Ok(())
}

// Directory that holds the repositories. Create it when it does not exist.
pub fn repositories_dir(cfg: &Config) -> PathBuf {
    let dir = if cfg.repositories_dir.trim().is_empty() {
        paths().repositories
    } else {
        PathBuf::from(&cfg.repositories_dir)
    };
    let _ = fs::create_dir_all(&dir);
    dir
}

// Where the conversations of the model would remain, from the text of the settings: the folder
// that was written, or the `contexts` folder inside the data folder. This makes nothing;
// `contexts_dir` below both reads and makes.
pub fn contexts_place(cfg: &Config) -> PathBuf {
    if cfg.contexts_dir.trim().is_empty() {
        paths().contexts
    } else {
        PathBuf::from(cfg.contexts_dir.trim())
    }
}

// The folder the contexts the model has processed are in: the read conversation of every chat
// that has the keeping of them switched on, which llama-server writes and reads back by name.
// Create it when it does not exist, as `repositories_dir` does. The conversations are the bulk
// of what a chat costs on a disk, which is why they may be put somewhere of their own, away
// from the rest of the files of the program.
pub fn contexts_dir(cfg: &Config) -> PathBuf {
    let dir = contexts_place(cfg);
    let _ = fs::create_dir_all(&dir);
    dir
}

// Search for the llama-server binary in the usual build places.
fn default_llama_dir() -> String {
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("AUTUMN_NATTER_LLAMA_DIR") {
        if !env.trim().is_empty() {
            tried.push(PathBuf::from(env));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            tried.push(dir.to_path_buf());
            tried.push(dir.join("llama"));
            tried.push(dir.join("../llamacpp/build/bin"));
            tried.push(dir.join("../../llamacpp/build/bin"));
        }
    }
    // A checkout next to the program is common during development.
    if let Ok(cwd) = std::env::current_dir() {
        tried.push(cwd.join("llamacpp/build/bin"));
    }
    tried.push(PathBuf::from("/usr/local/bin"));
    tried.push(PathBuf::from("/usr/bin"));
    for dir in tried {
        if server_in(&dir).is_some() {
            return dir.display().to_string();
        }
    }
    String::new()
}

// Return the llama-server binary inside a directory, if it is there.
pub fn server_in(dir: &Path) -> Option<PathBuf> {
    let names = [
        "llama-server",
        "llama-server.exe",
        "bin/llama/llama-server",
        "bin/llama/llama-server.exe",
        "bin/llama-server",
        "bin/llama-server.exe",
    ];
    for name in names {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // The word of a theme: the three words remain, a word that is none of them is read as the
    // pale theme, and an empty word -- the marker of a settings file written before there were
    // three -- answers with the switch of the two that such a file holds, so a chat that was on
    // the dark theme does not start up pale. A new set of settings holds no word of its own,
    // which is read as the pale theme.
    #[test]
    fn the_word_of_a_skin_is_kept_or_read_as_the_pale_one() {
        assert_eq!(theme_word(THEME_LIGHT, false), "light");
        assert_eq!(theme_word(THEME_DARK, false), "dark");
        assert_eq!(theme_word(THEME_CORPORATE, true), "corporate");
        assert_eq!(theme_word(THEME_SILVER, false), "silver");
        assert_eq!(theme_word(THEME_NEON, false), "neon");
        assert_eq!(theme_word("", true), "dark");
        assert_eq!(theme_word("", false), "light");
        assert_eq!(theme_word("shiny_and_blue", true), "light");
        let fresh = Config::default();
        assert_eq!(theme_word(&fresh.theme, fresh.dark_mode), THEME_LIGHT);
        // The list the pages step through holds every theme the program knows, once each, and
        // all of them is a word the settings panel can put on the page.
        assert_eq!(THEMES.len(), 5);
        for word in THEMES {
            assert_eq!(theme_word(word, false), word, "{} is a theme", word);
        }
    }

    // The pixel-art form belongs to the theme and not to a chat, so it is one word of the
    // settings: it is kept through a save and a read, a file written before it means the
    // standard form, and no other setting moves with it.
    #[test]
    fn the_pixel_art_form_of_a_skin_is_kept_with_the_skin() {
        let mut cfg = Config::default();
        assert!(!cfg.theme_pixel, "a new set of settings is drawn as usual");
        cfg.theme = THEME_NEON.to_string();
        cfg.theme_pixel = true;
        let text = serde_json::to_string(&cfg).unwrap_or_default();
        let back: Config = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back.theme, THEME_NEON);
        assert!(back.theme_pixel, "the form is read back as it was written");
        // A file of the program written before the pixel-art form existed holds no word of it,
        // and is read as the standard form.
        let mut written = serde_json::to_value(&cfg).unwrap_or(serde_json::Value::Null);
        if let Some(map) = written.as_object_mut() {
            map.remove("theme_pixel");
        }
        let older: Config = serde_json::from_value(written).unwrap_or_default();
        assert_eq!(older.theme, THEME_NEON, "the theme of the file is read");
        assert!(!older.theme_pixel);
    }

    // How the counting of the sources runs. A new set of settings keeps it on the cores of the
    // computer; a settings file written before these four words existed is read as those
    // defaults; and a file that names numbers out of their bounds is taken back into them, so
    // the server is never started with arguments it would refuse.
    #[test]
    fn the_running_of_the_counting_is_kept_in_the_settings() {
        let fresh = Config::default();
        assert!(fresh.embedding_cpu_only, "the card is left to the answers by default");
        assert_eq!(fresh.embedding_batch, crate::embed::RUN_BATCH);
        assert_eq!(fresh.embedding_threads, crate::embed::RUN_THREADS);
        assert_eq!(fresh.embedding_ctx, crate::embed::RUN_CTX);
        // A file of the program written before the running could be chosen holds no word of it.
        let mut written = serde_json::to_value(&fresh).unwrap_or(serde_json::Value::Null);
        if let Some(map) = written.as_object_mut() {
            for word in [
                "embedding_cpu_only",
                "embedding_batch",
                "embedding_threads",
                "embedding_ctx",
            ] {
                map.remove(word);
            }
        }
        let older: Config = serde_json::from_value(written).unwrap_or_default();
        assert!(older.embedding_cpu_only, "an older file keeps the counting off the card");
        assert_eq!(older.embedding_batch, crate::embed::RUN_BATCH);
        assert_eq!(older.embedding_threads, crate::embed::RUN_THREADS);
        assert_eq!(older.embedding_ctx, crate::embed::RUN_CTX);
        // The four numbers read out of the settings are the bounds clamped, which is what
        // `load` does to every settings file it reads.
        let wild = Config {
            embedding_cpu_only: false,
            embedding_batch: 9_000_000,
            embedding_threads: 4000,
            embedding_ctx: 100_000,
            ..Config::default()
        };
        let run = crate::embed::Run::of(&wild);
        assert!(!run.cpu_only, "the choice of the card is kept as it was written");
        assert_eq!(run.batch, crate::embed::BATCH_MAX);
        assert_eq!(run.threads, crate::embed::THREAD_MAX);
        assert_eq!(run.ctx, crate::embed::CTX_MAX);
    }

    // The word `corporate` meant the ladder of grey before the near-black theme took the name.
    // A file that holds it without the marker of the four themes is such a file, and is read as
    // `silver`; a file with the marker holds the word for the new theme and is skipped.
    #[test]
    fn the_older_word_for_the_ladder_of_grey_is_turned_once() {
        let mut old = Config {
            theme: THEME_CORPORATE.to_string(),
            skins_v2: false,
            ..Config::default()
        };
        assert!(turn_the_skin_words(&mut old), "the file was turned");
        assert_eq!(old.theme, THEME_SILVER);
        assert!(old.skins_v2);
        // The same file, read again after it was written with the marker on it, remains as it
        // is: the word now names the near-black theme.
        let mut newer = Config {
            theme: THEME_CORPORATE.to_string(),
            skins_v2: true,
            ..Config::default()
        };
        assert!(!turn_the_skin_words(&mut newer), "nothing to turn");
        assert_eq!(newer.theme, THEME_CORPORATE);
        // A file that held another theme keeps it.
        let mut dark = Config {
            theme: THEME_DARK.to_string(),
            skins_v2: false,
            ..Config::default()
        };
        assert!(turn_the_skin_words(&mut dark));
        assert_eq!(dark.theme, THEME_DARK);
    }

    // The name a preset is kept under: the text the user wrote, with the space off the ends and
    // the space inside made even, and cut to the length the menu of the presets holds. A name
    // of nothing at all returns as nothing, which is how the caller learns that there was no
    // name there.
    #[test]
    fn the_name_of_a_preset_is_made_even() {
        assert_eq!(sanitize_preset_name("  the   long answers  "), "the long answers");
        assert_eq!(sanitize_preset_name("\tnew   chat   card\t"), "new chat card");
        assert_eq!(sanitize_preset_name("   "), "");
        assert_eq!(sanitize_preset_name(""), "");
        // A name longer than the menu holds is cut to it, and stays one name.
        let long = format!("{} and more words beyond the name", "a".repeat(80));
        let cut = sanitize_preset_name(&long);
        assert_eq!(cut.chars().count(), GREATEST_PRESET_NAME);
        // A new set of settings holds no preset at all.
        assert!(Config::default().model_presets.is_empty());
    }

    // The switches of the Chatting section are on for a new set of settings, and the two
    // spaces for steps remain unlimited.
    #[test]
    fn the_chatting_switches_stand_on() {
        let cfg = Config::default();
        assert!(cfg.chat_collapse_thinking);
        assert!(cfg.chat_grow_thinking);
        assert!(cfg.chat_tools_in_thinking);
        assert!(cfg.chat_collapse_tools);
        assert!(cfg.chat_date_new_chat);
        assert!(cfg.chat_highlight);
        assert!(cfg.chat_colour_text);
        assert!(cfg.chat_chapters);
        assert!(cfg.chat_userline);
        assert!(cfg.chat_grid);
        assert_eq!(cfg.chat_background, BACKGROUND_NONE);
        assert!(cfg.chat_rain_splash);
        assert!(!cfg.chat_cursor_digits);
        assert!(cfg.chat_send_anim);
        assert!(cfg.chat_send_anim);
        assert!(cfg.chat_context_files);
        // The compressed form is requested, not given: a config that shows nothing keeps the
        // files as they have always been written.
        assert!(!cfg.chat_context_compression);
        assert_eq!(cfg.chat_max_tool_steps, -1);
        assert_eq!(cfg.chat_max_answer_steps, -1);
    }

    // The two folders a user may point elsewhere. The settings answer with the place the
    // program chose when they say nothing, and with the folder that was written when they name
    // one; text of spaces is nothing to reports.
    #[test]
    fn the_two_folders_of_the_settings_are_read_or_made() {
        let plain = Config::default();
        assert!(plain.data_dir.is_empty(), "a new set of settings names no folder");
        assert!(plain.contexts_dir.is_empty());
        assert_eq!(contexts_place(&plain), paths().contexts);
        // The folder named is the folder used, and it is created when it is requested, so a
        // folder written in the settings panel is a folder the program can write into.
        let _ = fs::remove_dir_all("uidata/folder-tests");
        let named = PathBuf::from("uidata/folder-tests/contexts");
        let moved = Config {
            contexts_dir: named.display().to_string(),
            ..Config::default()
        };
        assert_eq!(contexts_place(&moved), named);
        assert_eq!(contexts_dir(&moved), named);
        assert!(named.is_dir(), "the folder was created with the request");
        let spaces = Config {
            contexts_dir: "   ".to_string(),
            ..Config::default()
        };
        assert_eq!(contexts_place(&spaces), paths().contexts);
        let _ = fs::remove_dir_all("uidata/folder-tests");
    }

    // The marker file that names another folder for the files of the program: it is kept a
    // folder that remains and not for text that name nothing, the current settings are copied
    // to a folder that holds none, and empty text takes the marker away again - which is the
    // way back to the usual place.
    #[test]
    fn a_mark_names_another_folder_for_the_files_of_the_program() {
        let root = PathBuf::from("uidata/mark-tests");
        let usual = root.join("usual");
        let other = root.join("other");
        let mark = usual.join(MARK_FILE_NAME);
        let settings = usual.join(SETTINGS_FILE_NAME);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&usual).expect("the usual place is created");
        fs::write(&settings, "{}").expect("the settings are written");
        // Text that name no folder is no place. The marker is not was, nothing is made of the
        // text, and the program keeps reading from where it read.
        let nonsense = Config {
            data_dir: other.display().to_string(),
            ..Config::default()
        };
        assert!(point_data_dir_at(&mark, &settings, &nonsense).is_err());
        assert!(!mark.exists(), "no marker means a folder that is not there");
        assert!(!other.exists(), "the program makes no folder out of the settings");
        // A folder that remains is a place: the marker names it, and the current settings are
        // copied there, so that a program starting again at the new place starts with the
        // settings it had rather than with a new set of them.
        fs::create_dir_all(&other).expect("the other folder is created");
        point_data_dir_at(&mark, &settings, &nonsense).expect("the mark is stood");
        assert_eq!(read_mark(&mark).as_deref(), Some(other.as_path()));
        assert!(other.join(SETTINGS_FILE_NAME).is_file(), "the settings came along");
        // Empty text takes the marker away, and taking a marker away that is not there is
        // nothing to do rather than an error.
        let back = Config {
            data_dir: "  ".to_string(),
            ..Config::default()
        };
        point_data_dir_at(&mark, &settings, &back).expect("the marker is removed");
        assert!(!mark.exists());
        assert!(point_data_dir_at(&mark, &settings, &back).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    // A marker file that names nothing the program can read is answered by the usual place: a
    // folder on a disk that is not there today, and a file of another program's own, leave the
    // program working rather than lost.
    #[test]
    fn a_mark_that_names_no_folder_is_no_place() {
        let root = PathBuf::from("uidata/mark-read-tests");
        let mark = root.join(MARK_FILE_NAME);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the folder is made");
        assert_eq!(read_mark(&mark), None, "no mark stands at all");
        fs::write(&mark, "  \n").expect("an empty marker is written");
        assert_eq!(read_mark(&mark), None);
        fs::write(&mark, "a folder that is not here\n").expect("a mark of words");
        assert_eq!(read_mark(&mark), None);
        fs::write(&mark, format!("{}\n", root.display())).expect("a marker of this folder");
        assert_eq!(read_mark(&mark).as_deref(), Some(root.as_path()));
        let _ = fs::remove_dir_all(&root);
    }

    // A settings file written before the two folders could be chosen still reads, with both of
    // them empty: the usual places answer, as they did before.
    #[test]
    fn an_old_settings_file_names_neither_folder() {
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert!(read.data_dir.is_empty());
        assert!(read.contexts_dir.is_empty());
        assert_eq!(contexts_place(&read), paths().contexts);
    }

    // A number of steps is kept and read back, and a number below -1 is read as no limit rather
    // than as a nonsense ceiling.
    #[test]
    fn the_rooms_for_steps_are_kept() {
        let cfg = Config {
            chat_max_tool_steps: 3,
            chat_max_answer_steps: 40,
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).expect("the settings are written");
        let back: Config = serde_json::from_str(&text).expect("the settings are read");
        assert_eq!(back.chat_max_tool_steps, 3);
        assert_eq!(back.chat_max_answer_steps, 40);
        // The engine reads a number as a ceiling, and -1 as no ceiling at all.
        assert_eq!(engine_steps(back.chat_max_tool_steps), Some(3));
        assert_eq!(engine_steps(-1), None);
        assert_eq!(engine_steps(-9), None);
        assert_eq!(engine_steps(0), Some(0));
    }

    // The defaults of the Sampling section remain as the program wants them.
    #[test]
    fn the_sampling_defaults_are_kept() {
        let s = ModelSettings::default();
        assert!((s.temperature - 0.7).abs() < f64::EPSILON);
        assert!((s.top_p - 0.95).abs() < f64::EPSILON);
        assert_eq!(s.top_k, 20);
        assert!((s.min_p - 0.0).abs() < f64::EPSILON);
        assert!((s.repeat_penalty - 1.0).abs() < f64::EPSILON);
        assert!((s.presence_penalty - 0.0).abs() < f64::EPSILON);
        assert_eq!(s.max_tokens, -1);
        assert_eq!(s.seed, -1);
        // The values are written in the short form, so the window shows 0.7 and not the full
        // form of a small number.
        let text = serde_json::to_string(&s).expect("the settings are written");
        assert!(text.contains("\"temperature\":0.7"));
        assert!(text.contains("\"top_p\":0.95"));
        // A file written before the section existed still reads, with the defaults of the
        // section in place.
        let old = "{\"ctx_size\":8192,\"kv_index\":1,\"spec_index\":0,\"draft_model\":\"\",\
                    \"draft_tokens\":4,\"parallel\":1,\"batch_size\":512,\"ubatch_size\":128,\
                    \"tensor_parallel\":false,\"vision\":false,\"reasoning_budget\":-1,\
                    \"reasoning_message\":\"\",\"chat_template\":\"\",\"load_index\":1,\
                    \"extra_args\":\"\",\"vram_budget\":0,\"gpu_enabled\":[]}";
        let read: ModelSettings =
            serde_json::from_str(old).expect("an old model settings entry reads");
        assert!((read.temperature - 0.7).abs() < f64::EPSILON);
        assert_eq!(read.top_k, 20);
        assert_eq!(read.max_tokens, -1);
        assert_eq!(read.seed, -1);
    }

    // A settings file written before the Chatting section existed still reads, and its switches
    // are on rather than off.
    #[test]
    fn an_old_settings_file_keeps_the_switches_on() {
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert!(read.chat_collapse_thinking);
        assert!(read.chat_grow_thinking);
        assert!(read.chat_tools_in_thinking);
        assert!(read.chat_collapse_tools);
        assert!(read.chat_date_new_chat);
        assert!(read.chat_highlight);
        assert!(read.chat_chapters);
        // The keeping of the conversations was not in the file either, and it reads on rather
        // than off: a user who never heard of it is not left with the chats read through again
        // on every coming back.
        assert!(read.chat_context_files);
        // The compressed form was not in the file either, and it reads off: a user who never
        // heard of it is not moved to the other form.
        assert!(!read.chat_context_compression);
        // The two spaces for steps were not in the file either, so they read as unlimited
        // rather than as a ceiling of nothing.
        assert_eq!(read.chat_max_tool_steps, -1);
        assert_eq!(read.chat_max_answer_steps, -1);
    }

    // A paste of text into the chat composer becomes a file at six thousand characters, and a
    // settings file written before this setting was reads with the same number. The number
    // keeps inside its two bounds.
    #[test]
    fn a_paste_of_words_becomes_a_file_after_six_thousand() {
        assert_eq!(Config::default().pasted_text_char_limit, 6000);
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert_eq!(read.pasted_text_char_limit, 6000);
        assert_eq!(sanitize_pasted_text_limit(9), 100);
        assert_eq!(sanitize_pasted_text_limit(5000), 5000);
        assert_eq!(sanitize_pasted_text_limit(2_000_000), 1_000_000);
    }

    // The width of the chat. The width the chat always had is the default and the least, so a
    // settings file written before this setting was keeps the chat as wide as it always was.
    // The field cannot go below that width, and cannot request a chat wider than a wall.
    #[test]
    fn the_width_of_the_chat_keeps_inside_its_bounds() {
        assert_eq!(Config::default().chat_width, LEAST_CHAT_WIDTH);
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert_eq!(read.chat_width, LEAST_CHAT_WIDTH);
        assert_eq!(sanitize_chat_width(400), LEAST_CHAT_WIDTH);
        assert_eq!(sanitize_chat_width(1100), 1100);
        assert_eq!(sanitize_chat_width(9000), GREATEST_CHAT_WIDTH);
    }

    // The Compaction section of the settings. A new set of settings requests the handoff
    // summary, by the share of the space, at 85 per cent, with the prompt of the program in
    // the text box, and with the timeline of the summarisations remaining beside the chat.
    #[test]
    fn the_handoff_summary_is_the_default_compaction() {
        let cfg = Config::default();
        assert_eq!(cfg.chat_compaction, COMPACTION_HANDOFF);
        assert_eq!(cfg.chat_compact_by, COMPACT_BY_PERCENT);
        assert_eq!(cfg.chat_compact_percent, 85);
        assert_eq!(cfg.chat_compact_reserve, 10240);
        assert!(cfg.chat_timeline);
        assert_eq!(cfg.chat_compact_prompt, DEFAULT_HANDOFF_PROMPT);
        // The prompt of the program is a text a model can be sent, and it is short enough to
        // fit the ceiling of the field.
        assert!(cfg.chat_compact_prompt.starts_with("Write a detailed"));
        // The prompt of the program names a length, for a summary that never ends leaves the
        // space it was asked to free.
        assert!(cfg.chat_compact_prompt.contains("three hundred words"));
        assert!(cfg.chat_compact_prompt.chars().count() <= GREATEST_HANDOFF_PROMPT);
        // The second ask has a prompt of its own, and it leans towards a short summary rather
        // than a long one.
        assert_eq!(cfg.chat_compact_prompt_short, DEFAULT_HANDOFF_PROMPT_SHORT);
        assert!(cfg.chat_compact_prompt_short.starts_with("Write a very short"));
        assert!(cfg
            .chat_compact_prompt_short
            .chars()
            .count()
            <= GREATEST_HANDOFF_PROMPT);
    }

    // A settings file written before the Compaction section existed still reads, and it reads
    // with the handoff summary requested rather than with nothing.
    #[test]
    fn an_old_settings_file_asks_for_the_handoff_summary() {
        let old = "{\"llama_dir\":\"\",\"models_dir\":\"\",\"repositories_dir\":\"\",\
                   \"dark_mode\":false,\"remote_port\":8723,\"upnp_enabled\":false,\
                   \"model_id\":\"\",\"model_settings\":{}}";
        let read: Config = serde_json::from_str(old).expect("an old settings file reads");
        assert_eq!(read.chat_compaction, COMPACTION_HANDOFF);
        assert_eq!(read.chat_compact_by, COMPACT_BY_PERCENT);
        assert_eq!(read.chat_compact_percent, 85);
        assert_eq!(read.chat_compact_reserve, 10240);
        assert_eq!(read.chat_compact_prompt, DEFAULT_HANDOFF_PROMPT);
        assert_eq!(
            read.chat_compact_prompt_short,
            DEFAULT_HANDOFF_PROMPT_SHORT
        );
        assert!(read.chat_timeline);
        assert!(read.chat_colour_text);
    }

    // The fields of the Compaction section are kept inside their bounds. A word that is neither
    // of the two compactions gives the handoff summary, and a prompt left empty gives the
    // prompt of the program again.
    #[test]
    fn the_compaction_fields_are_kept_inside_their_bounds() {
        assert_eq!(sanitize_compaction("off"), COMPACTION_OFF);
        assert_eq!(sanitize_compaction("handoff"), COMPACTION_HANDOFF);
        assert_eq!(sanitize_compaction("a word of the user"), COMPACTION_HANDOFF);
        assert_eq!(sanitize_compact_by("tokens"), COMPACT_BY_TOKENS);
        assert_eq!(sanitize_compact_by("percent"), COMPACT_BY_PERCENT);
        assert_eq!(sanitize_compact_by(""), COMPACT_BY_PERCENT);
        assert_eq!(sanitize_compact_percent(0), 1);
        assert_eq!(sanitize_compact_percent(72), 72);
        assert_eq!(sanitize_compact_percent(400), 100);
        assert_eq!(sanitize_compact_reserve(0), 1);
        assert_eq!(sanitize_compact_reserve(10240), 10240);
        assert_eq!(sanitize_compact_reserve(9_000_000), 1_000_000);
        // The prompt of the user is kept, and a long one is cut at the ceiling rather than
        // refused.
        assert_eq!(sanitize_handoff_prompt("say something short"), "say something short");
        let long = "x".repeat(GREATEST_HANDOFF_PROMPT + 500);
        assert_eq!(
            sanitize_handoff_prompt(&long).chars().count(),
            GREATEST_HANDOFF_PROMPT
        );
        assert_eq!(sanitize_handoff_prompt("   "), DEFAULT_HANDOFF_PROMPT);
        // The prompt of the second ask is kept by the same rule, and a field left empty gives
        // the short prompt of the program again.
        assert_eq!(
            sanitize_handoff_prompt_short("say very little"),
            "say very little"
        );
        assert_eq!(
            sanitize_handoff_prompt_short(&long).chars().count(),
            GREATEST_HANDOFF_PROMPT
        );
        assert_eq!(
            sanitize_handoff_prompt_short("  "),
            DEFAULT_HANDOFF_PROMPT_SHORT
        );
    }

    // The text before the conversation. The prompt of the user is kept with its own text and
    // cut at its own ceiling, and the space about it is taken off, so a box of nothing but
    // spaces is no prompt.
    #[test]
    fn the_system_prompt_of_the_user_is_kept_and_cut_at_its_ceiling() {
        assert_eq!(sanitize_system_prompt("Answer in short sentences."), "Answer in short sentences.");
        assert_eq!(sanitize_system_prompt("  \n  Praise nothing.  \n "), "Praise nothing.");
        assert_eq!(sanitize_system_prompt("   "), "");
        let long = "x".repeat(GREATEST_SYSTEM_PROMPT + 400);
        assert_eq!(
            sanitize_system_prompt(&long).chars().count(),
            GREATEST_SYSTEM_PROMPT
        );
        // A new set of settings is with no prompt of the user and no marker on a pre-written
        // prompt, so a chat reads as it read before these were.
        let fresh = Config::default();
        assert_eq!(fresh.chat_system_prompt, "");
        assert!(fresh.chat_system_prompts_on.is_empty());
    }

    // The markers on the pre-written prompts. A word the program does not know is dropped, a
    // marker given twice is kept once, and the order kept is the order of the table and not the
    // order the window sent.
    #[test]
    fn the_marks_on_the_pre_written_prompts_are_kept_in_the_order_of_the_table() {
        let known: Vec<String> = crate::prompts::lines()
            .iter()
            .map(|one| String::from(one["id"].as_str().unwrap_or("")))
            .collect();
        assert!(!known.is_empty(), "the table of prompts is not empty");
        let last = known.last().expect("the last id of the table");
        let first = known.first().expect("the first id of the table");
        // Both markers, sent in the wrong order, return in the order of the table, and a marker
        // sent twice remains once.
        let sent = vec![
            String::from(last),
            String::from("no-such-prompt"),
            String::from(first),
            String::from(first),
        ];
        let kept = sanitize_prompt_ids(&sent);
        assert_eq!(kept, vec![String::from(first), String::from(last)]);
        assert!(sanitize_prompt_ids(&[]).is_empty());
        assert_eq!(sanitize_prompt_ids(&sent).len(), kept.len());
    }

    // The rule that sets the summarisation going. The share of the space works from the
    // current conversation, the space left works from what is over, and a conversation past the
    // space is past either threshold.
    #[test]
    fn the_summarisation_starts_at_the_share_asked_for_or_at_the_room_left() {
        let by_percent = Config {
            chat_compact_by: String::from(COMPACT_BY_PERCENT),
            chat_compact_percent: 85,
            ..Config::default()
        };
        assert!(!compaction_wanted(&by_percent, 4096, 8192));
        assert!(!compaction_wanted(&by_percent, 6962, 8192));
        assert!(compaction_wanted(&by_percent, 6964, 8192));
        assert!(compaction_wanted(&by_percent, 8192, 8192));
        assert!(compaction_wanted(&by_percent, 9000, 8192));
        // Where the limit remains is the least space that is answered as past it, so an item
        // paid for out of the space can be measured against it.
        assert_eq!(compaction_room(&by_percent, 8192), 6964);
        // Nothing is known of the space, so nothing is reported about running out.
        assert!(!compaction_wanted(&by_percent, 4000, 0));
        assert_eq!(compaction_room(&by_percent, 0), u64::MAX);

        let by_tokens = Config {
            chat_compact_by: String::from(COMPACT_BY_TOKENS),
            chat_compact_reserve: 10240,
            ..Config::default()
        };
        // A space of 32 768 with 10 000 left over is past the least left over.
        assert!(compaction_wanted(&by_tokens, 22768, 32768));
        // With 12 000 left over there is still space for the next answer.
        assert!(!compaction_wanted(&by_tokens, 20768, 32768));
        // The number of the share does not matter on this path.
        assert!(compaction_wanted(&by_tokens, 7000, 8192));
        // The space left over is the limit on this path: 32 768 with 10 240 kept.
        assert_eq!(compaction_room(&by_tokens, 32768), 22528);

        // No compaction at all, whatever the conversation.
        let off = Config {
            chat_compaction: String::from(COMPACTION_OFF),
            ..Config::default()
        };
        assert!(!compaction_wanted(&off, 9000, 8192));
        assert!(!compaction_wanted(&off, 8192, 8192));
        // With the summarisation switched off no limit is set, so all of the space is free.
        assert_eq!(compaction_room(&off, 8192), u64::MAX);

        // The line of the summarisation, for the context meter, is at the same place as the
        // limit. It remains nowhere at all when there is no summarisation, or nothing known of
        // the space, or a place beyond all of the space, or no space left to speak of.
        assert_eq!(compaction_mark(&by_percent, 8192), Some(6964));
        assert_eq!(compaction_mark(&by_tokens, 32768), Some(22528));
        assert_eq!(compaction_mark(&by_percent, 0), None);
        assert_eq!(compaction_mark(&off, 8192), None);
        let the_whole = Config {
            chat_compact_by: String::from(COMPACT_BY_PERCENT),
            chat_compact_percent: 100,
            ..Config::default()
        };
        assert_eq!(compaction_mark(&the_whole, 8192), None);
        let none_left = Config {
            chat_compact_by: String::from(COMPACT_BY_TOKENS),
            chat_compact_reserve: 9000,
            ..Config::default()
        };
        assert_eq!(compaction_mark(&none_left, 8192), None);
    }

    // The background of a chat is one of the names the page can draw. A word written another
    // way is accepted, and a word the page cannot draw - from an older or a newer window, or
    // from a pointer in the settings file - leaves the chat on the background it always had.
    #[test]
    fn a_background_the_page_cannot_draw_is_no_background() {
        assert_eq!(sanitize_background(BACKGROUND_LEAVES), BACKGROUND_LEAVES);
        assert_eq!(sanitize_background("  Leaves  "), BACKGROUND_LEAVES);
        assert_eq!(sanitize_background(BACKGROUND_RAIN), BACKGROUND_RAIN);
        assert_eq!(sanitize_background(" RAIN "), BACKGROUND_RAIN);
        assert_eq!(sanitize_background("SNOW"), BACKGROUND_NONE);
        assert_eq!(sanitize_background(""), BACKGROUND_NONE);
        assert_eq!(Config::default().chat_background, BACKGROUND_NONE);
        assert!(Config::default().chat_rain_splash);
        assert!(!Config::default().chat_cursor_digits);
        assert!(Config::default().chat_grid);
    }
}
