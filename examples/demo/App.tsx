// The shell: the root, the theme's background and the navigation stack. Product screens land in the second pass.
import { mount, View } from "nsh";
import { NavPage, NavStack } from "nsh/base";
import { Header } from "./components/Header";
import { nav, type Route } from "./nav";
import { productEntry } from "./src/products";
import { AnalyticsScreen } from "./src/screens/analytics";
import { AuthScreen } from "./src/screens/auth";
import { DatabaseScreen } from "./src/screens/database";
import { HomeScreen } from "./src/screens/home";
import { MessagingScreen } from "./src/screens/messaging";
import { MonitorScreen } from "./src/screens/monitor";
import { StorageScreen } from "./src/screens/storage";

const HOME_TITLE = "Firebase demo";

function pageTitle(name: Route["name"]): string {
  return name === "home" ? HOME_TITLE : productEntry(name).title;
}

function screen(route: Route) {
  switch (route.name) {
    case "home":
      return <HomeScreen />;
    case "auth":
      return <AuthScreen />;
    case "database":
      return <DatabaseScreen />;
    case "storage":
      return <StorageScreen />;
    case "messaging":
      return <MessagingScreen />;
    case "analytics":
      return <AnalyticsScreen />;
    case "monitor":
      return <MonitorScreen />;
  }
}

function App() {
  return (
    <View class="flex-1 bg-background">
      <NavStack state={nav} class="flex-1">
        {(route) => (
          <NavPage
            title={pageTitle(route.name)}
            testID={`page-${route.name}`}
            header={<Header title={pageTitle(route.name)} />}
          >
            {screen(route)}
          </NavPage>
        )}
      </NavStack>
    </View>
  );
}

mount(App);
