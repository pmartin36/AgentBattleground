use std::io;

use scene_core::scene_id::SceneId;

fn main() -> io::Result<()> {
    game::run(game::registry::construct(SceneId::MainHub))
}
