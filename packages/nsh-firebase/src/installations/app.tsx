// The tests' entry: it mounts the exercise UI from tests/, and this
// directory's nsh.json is what grants the app its firebase.* host names
// and the installations REST host (nsh reads the nsh.json next to the
// entry, and grants host functions and network hosts never without a
// declaration).
import { mount } from "nsh";
import { InstallationsExerciseApp } from "../../tests/installations-app";

mount(InstallationsExerciseApp);
