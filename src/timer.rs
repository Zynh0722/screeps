use log::info;
use screeps::game;

pub(crate) struct TimerLog {
    pub(crate) name: String,
    pub(crate) loaded: f64,
}

impl TimerLog {
    pub(crate) fn start(name: String) -> Self {
        Self {
            name,
            loaded: game::cpu::get_used(),
        }
    }
}

impl Drop for TimerLog {
    fn drop(&mut self) {
        info!(
            "\n{} done!\n\t| Init. At: {:.2}cpu\n\t| Added: {:.2}cpu",
            self.name,
            self.loaded,
            game::cpu::get_used() - self.loaded
        )
    }
}
