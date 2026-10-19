//! Media playback options survive a PPTX save and reopen (issue #74): volume, loop, rewind, play
//! across slides, hide while not playing, full screen and Start: Automatically.

use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{MediaClip, Presentation, Shape, ShapeId, ShapeKind, defaults};

fn clip_shape(p: &mut Presentation, name: &str, clip: MediaClip) -> Shape {
    Shape {
        id: ShapeId(p.alloc_id().expect("id")),
        name: name.into(),
        xfrm: Some(Xfrm::new(40.0, 40.0, 80.0, 80.0)),
        kind: ShapeKind::Media(clip),
        ..Default::default()
    }
}

fn clips(p: &Presentation) -> Vec<(String, MediaClip)> {
    p.slides[0]
        .shapes
        .iter()
        .filter_map(|s| match &s.kind {
            ShapeKind::Media(m) => Some((s.name.clone(), m.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn playback_options_round_trip() {
    let mut p = defaults::new_presentation(None);
    let audio = p.add_media("a.m4a", "audio/mp4", b"audio-bytes".to_vec()).expect("media");
    let video = p.add_media("v.mp4", "video/mp4", b"video-bytes".to_vec()).expect("media");
    let song = clip_shape(
        &mut p,
        "Song",
        MediaClip {
            media: audio,
            volume: 0.3,
            autoplay: true,
            loop_play: true,
            rewind: true,
            play_across_slides: true,
            hide_while_not_playing: true,
            ..Default::default()
        },
    );
    let film = clip_shape(&mut p, "Film", MediaClip { media: video, video: true, volume: 0.0, full_screen: true, ..Default::default() });
    let plain = clip_shape(&mut p, "Plain", MediaClip { media: audio, volume: 1.0, ..Default::default() });
    let s = Arc::make_mut(&mut p.slides[0]);
    s.shapes.extend([song, film, plain]);
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let q = deckcraft_pptx::import(&bytes).expect("import");
    let got = clips(&q);
    let find = |n: &str| got.iter().find(|(name, _)| name == n).map(|(_, c)| c.clone()).expect("clip");
    let song = find("Song");
    assert!((song.volume - 0.3).abs() < 1e-3, "{song:?}");
    assert!(song.autoplay && song.loop_play && song.rewind && song.play_across_slides && song.hide_while_not_playing, "{song:?}");
    assert!(!song.full_screen);
    let film = find("Film");
    assert!(film.volume.abs() < 1e-9 && film.full_screen, "{film:?}");
    assert!(!film.autoplay && !film.loop_play && !film.rewind && !film.hide_while_not_playing, "{film:?}");
    let plain = find("Plain");
    assert!((plain.volume - 1.0).abs() < 1e-9, "{plain:?}");
    assert!(!plain.autoplay && !plain.loop_play && !plain.hide_while_not_playing, "{plain:?}");
    // Start: Automatically is the media's own Play effect, not an extra Animation Pane row.
    assert!(q.slides[0].animations.is_empty(), "{:?}", q.slides[0].animations);
    // A second save of the reopened deck keeps the options.
    let r = deckcraft_pptx::import(&deckcraft_pptx::export(&q).expect("export")).expect("import");
    assert_eq!(
        clips(&r).iter().map(|(_, c)| (c.autoplay, c.loop_play, c.rewind)).collect::<Vec<_>>(),
        got.iter().map(|(_, c)| (c.autoplay, c.loop_play, c.rewind)).collect::<Vec<_>>()
    );
}

/// The clip's media node begins on its Play effect, not on a Pause or Stop that comes first.
#[test]
fn media_node_begins_on_the_play_effect_only() {
    use deckcraft_model::anim::{AnimClass, AnimStart, Animation};
    use std::io::{Cursor, Read};
    let xml = |effects: &[&str]| {
        let mut p = defaults::new_presentation(None);
        let audio = p.add_media("a.m4a", "audio/mp4", b"audio-bytes".to_vec()).expect("media");
        // A looping clip, so the writer emits its media node.
        let clip = clip_shape(&mut p, "Song", MediaClip { media: audio, volume: 1.0, loop_play: true, ..Default::default() });
        let id = clip.id;
        let s = Arc::make_mut(&mut p.slides[0]);
        s.shapes.push(clip);
        for e in effects {
            s.animations.push(Animation { shape: id, class: AnimClass::Media, effect: (*e).into(), start: AnimStart::OnClick, ..Default::default() });
        }
        let bytes = deckcraft_pptx::export(&p).expect("export");
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
        let mut xml = String::new();
        z.by_name("ppt/slides/slide1.xml").expect("slide").read_to_string(&mut xml).expect("xml");
        xml
    };
    // The id the media node's begin condition names, if any.
    let begin = |xml: &str| -> Option<String> {
        let node = xml.split("<p:cMediaNode").nth(1)?.split("</p:cMediaNode>").next()?;
        let after = node.split(r#"evt="onBegin""#).nth(1)?;
        Some(after.split(r#"<p:tn val=""#).nth(1)?.split('"').next()?.to_string())
    };
    assert!(xml(&["pause"]).contains("<p:cMediaNode"), "the node is written");
    assert_eq!(begin(&xml(&["pause"])), None, "a clip with only a Pause effect begins on nothing");
    assert_eq!(begin(&xml(&["stop", "pause"])), None);
    // Pause first, Play second: the node begins on the Play effect's node.
    let both = xml(&["pause", "play"]);
    let tn = begin(&both).expect("begins on the Play effect");
    // The effect node that holds the playFrom call is the last mediacall node opened before it.
    let before_play = &both[..both.find("playFrom(0.0)").unwrap()];
    let effect = before_play.rsplit(r#"presetClass="mediacall""#).nth(1).unwrap();
    let id_of_effect = effect.rsplit(r#"<p:cTn id=""#).next().unwrap().split('"').next().unwrap().to_string();
    assert_eq!(tn, id_of_effect, "{both}");
    assert!(begin(&xml(&["play"])).is_some());
}
