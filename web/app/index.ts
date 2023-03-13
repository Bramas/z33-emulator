import "./style.css";
import bindings from "z33-web-bindings";

import AnsiConverter from 'ansi-to-html';
const ansiConverter = new AnsiConverter();

const samples = {
  directives: () => import("../../samples/directives.S"),
  fact: () => import("../../samples/fact.S"),
  handler: () => import("../../samples/handler.S"),
};

type Output = {
  preprocessed?: Array<[number, string]>;
  error?: string;
  memory?: Array<[number, string]>;
  registers?: string;
  instructions?: Array<string>;
};

const createSection = (title: string, parent: Element): HTMLOutputElement => {
  const section = document.createElement("section");
  const heading = document.createElement("h4");
  heading.appendChild(document.createTextNode(title));
  const output = document.createElement("output");
  section.appendChild(heading);
  section.appendChild(output);
  parent.appendChild(section);
  return output;
};


const createErrorSection = (title: string, parent: Element): HTMLDivElement => {
  const section = document.createElement("section");
  const heading = document.createElement("h4");
  heading.appendChild(document.createTextNode(title));
  const output = document.createElement("div");
  section.appendChild(heading);
  section.appendChild(output);
  parent.appendChild(section);
  return output;
};

(async () => {
  const root = document.createElement("main");

  const editorContainer = document.createElement("div");
  editorContainer.classList.add("editor-container");
  editorContainer.classList.add("loading");
  root.appendChild(editorContainer);
  const result = document.createElement("pre");
  result.classList.add("result");
  root.appendChild(result);

  const consoleOutput = createErrorSection("Console", result);
  const memoryOutput = createSection("Memory", result);
  const instructionsOutput = createSection("Instructions", result);
  const preprocessorOutput = createSection("Preprocessor", result);
  consoleOutput.innerHTML = "Loading compiler...";

  document.body.appendChild(root);

  const { dump } = await bindings();

  const monaco = await import("./monaco");
  editorContainer.classList.remove("loading");

  const model = monaco.editor.createModel("", "text/plain");

  const selector = document.createElement("div");
  selector.classList.add("selector");
  editorContainer.appendChild(selector);
  selector.appendChild(document.createTextNode("Load sample: "));

  Object.entries(samples).forEach(([key, load]) => {
    const button = document.createElement("button");
    button.appendChild(document.createTextNode(key));
    button.addEventListener("click", async () => {
      const program = await load();
      model.setValue(program.default);
    });
    button.addEventListener("mouseover", () => load()); // Preload on mouseover
    selector.appendChild(button);
  });

  const editor = document.createElement("div");
  editor.classList.add("editor");
  editorContainer.appendChild(editor);

  monaco.editor
    .create(editor, {
      automaticLayout: true,
      theme: "vs-dark",
    })
    .setModel(model);

  const update = () => {
    const value = model.getValue();
    const output: Output = dump(value);
    if(output.error) {
      let v = output.error.replace(/\\u\{1b\}/g, "\\x1b");
      consoleOutput.innerHTML = ansiConverter.toHtml(v);
    } else {
      let v = (output.registers || '-').replace(/\\u\{1b\}/g, "\\x1b");
      consoleOutput.innerHTML = v;
    }

    preprocessorOutput.value = (output.preprocessed
      ? output.preprocessed.map(([k, v]) => `${k}\t${v}`).join("\n")
      : "-"
    );

    instructionsOutput.value = (output.instructions || ["-"]).join("\n");
    memoryOutput.value = (output.memory
      ? output.memory.map(([k, v]) => `${k}:\t${v}`).join("\n") 
      : "-"
    );
  };

  model.onDidChangeContent(() => update());

  update();
})();
