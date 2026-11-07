// The tests' entry: it mounts the exercise UI from tests/, and this
// directory's nsh.json is what grants the app its firebase.* host names
// (nsh reads the nsh.json next to the entry, and grants host functions
// never without a declaration).
import { mount } from "nsh";
import { MessagingExerciseApp } from "../../tests/messaging-app";

mount(MessagingExerciseApp);
