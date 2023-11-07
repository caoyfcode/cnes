fn main() {
    // cnes::run("test_roms/NES-NROM-128/pacman.nes");
    // cnes::run("test_roms/NES-NROM-128/Balloon Fight.nes");
    // cnes::run("test_roms/NES-NROM-128/Golf.nes");
    // cnes::run("test_roms/NES-NROM-128/Tennis.nes");
    // cnes::run("test_roms/NES-NROM-128/Ice Climber.nes");
    cnes::run("test_roms/NES-NROM-128/F-1 Race.nes"); // render bug, 可以实现弯道显示, 但是锯齿严重
    // cnes::run("test_roms/NES-NROM-128/Baseball.nes");
    // cnes::run("test_roms/NES-NROM-128/Bomberman.nes"); // fail
    // cnes::run("test_roms/NES-NROM-256/1942.nes");
    // cnes::run("test_roms/NES-NROM-256/Super Mario Bros.nes"); // render bug, sprite0hit未实现导致HUD问题, sprite与background遮挡关系也未算
    // cnes::run("test_roms/NES-NROM-256/10-Yard Fight.nes");
    // cnes::run("test_roms/NES-NROM-256/Volleyball.nes");1
}
