import React from "react";
import { createRoot } from "react-dom/client";
import { SessionsPage } from "./pages/SessionsPage";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SessionsPage />
  </React.StrictMode>
);
