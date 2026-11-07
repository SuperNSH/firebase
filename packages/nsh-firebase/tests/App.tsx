// The app entry for tests opened under tests/ (its nsh.json): mounts the
// same analytics exercise UI the tests drive. The package-root App.tsx
// stays the core area's entry; tests opening `../App.tsx` keep running
// under the root's nsh.json.
import "./analytics-app";
