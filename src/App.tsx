import Practice from "./features/practice/Practice";
import { createPracticeClient } from "./lib/tauri";

// O cliente não abre áudio nem IPC ao ser criado; cada ação chama o comando
// Tauri correspondente. A injeção permite testar a interface isoladamente.
const client = createPracticeClient();

export default function App() {
  return <Practice client={client} />;
}
