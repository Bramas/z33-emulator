
const Instructions = [
    /// Add a value to a register
    'add',
    /// Bitwise `and` with a given value
    'and',
    /// Push `%pc` and go to the given address
    'call',
    /// Compare a value with a register
    'cmp',
    /// Divide a register by a value
    'div',
    /// Load a memory cell to a register and set this cell to 1
    'fas',
    /// Read a value from an I/O controller
    'in',
    /// Unconditional jump
    'jmp',
    /// Jump if equal
    'jeq',
    /// Jump if not equal
    'jne',
    /// Jump if less or equal
    'jle',
    /// Jump if strictly less
    'jlt',
    /// Jump if greater of equal
    'jge',
    /// Jump if strictly greater
    'jgt',
    /// Load a register with a value
    'ld',
    /// Multiply a value to a register
    'mul',
    'neg',
    /// No-op
    'nop',
    /// Bitwise negation of a register
    'not',
    /// Bitwise `or` with a given value
    'or',
    /// Write a value to an I/O controller
    'out',
    /// Pop a value from the stack
    'pop',
    /// Push a value into the stack
    'push',
    /// Reset the computer
    'reset',
    /// Return from an interrupt or an exception
    'rti',
    /// Return from a `call`
    'rtn',
    /// Bitshift to the left
    'shl',
    /// Bitshift to the right
    'shr',
    /// Store a register value in memory
    'st',
    /// Substract a value from a register
    'sub',
    /// Swap a value and a register
    'swap',
    /// Start a `trap` exception
    'trap',
    /// Bitwise `xor` with a given value
    'xor',
]






export default function(monaco) {
    monaco.languages.register({ id: "z33" });

       
    // Register a tokens provider for the language
    monaco.languages.setMonarchTokensProvider("z33", {
		includeLF: true,
        tokenizer: {
            root: [
                [new RegExp('\\s('+Instructions.join('|')+')\\b'), "command", "@command"],
                [new RegExp('^('+Instructions.join('|')+')\\b'), "command", "@command"],
                [/[\w_]+:/, "label"],
                [/#\w+/, "macro"],
                [/\.\w+/, "macro"],
                {include: '@constants'},
                {include: '@comments'},
            ],
            command: [
                [/\n$/, "endofline", "@pop"],
                [/(\w|%)+[^a-zA-Z0-9_,\/]+(\w|%)+/, "error-missing-comma", "@pop"],
                [/\[[^\]]*\][^a-zA-Z0-9_,\/]+(\w|%)+/, "error-missing-comma", "@pop"],
                [/(\w|%)+[^a-zA-Z0-9_,\/]+\[[^\]]*\]/, "error-missing-comma", "@pop"],
                [/\[/,"operator", "@idx"],
                [/,/,"operator"],
                {include: '@constants'},
                {include: '@comments'},
                {include: '@registers'},
            ],
            constants: [
                [/[0-9]+/, "number"],
            ],
            comments: [
                [/\/\/.*/, "comment"],
            ],
			registers: [
                [/%(a|b|sp|pc)\b/, "register"],
                [/%[^ab]\b/, "error"],
                [/%s[^p]/, "error"],
                [/%p[^c]/, "error"],
                [/%[^absp]/, "error"],
			],
			idx: [
				[/\]/, "operator", "@pop"],
				[/[^a-zA-Z0-9\-+% \t\]][^\n]*/, {token: "error", next: '@pop'} ],
                {include: '@constants'},
                {include: '@registers'},
			],
        },
    });
    monaco.editor.defineTheme("z33-theme", {
        base: "vs-dark",
        inherit: true,
        rules: [
            { token: "number", foreground: "d98fca", fontStyle: "bold" },
            { token: "command", foreground: "ffffff", fontStyle: "bold" },
            { token: "register", foreground: "7693d9", fontStyle: "bold" },
            { token: "label", foreground: "ffc14f"},
            { token: "comment", foreground: "737373"},
            { token: "macro", foreground: "826a51"},
            { token: "operator", foreground: "b9b900"},
            { token: "error", foreground: "ff0000"},
			{ token: "error-missing-comma", foreground: "ff0000"},
        ],
        colors: {
            //"editor.foreground": "#000000",
        },
    });
    
    //not working but I don't know why
    monaco.languages.registerCompletionItemProvider("z33", {
        provideCompletionItems: (model, position) => {
            var word = model.getWordUntilPosition(position);
            var range = {
                startLineNumber: position.lineNumber,
                endLineNumber: position.lineNumber,
                startColumn: word.startColumn,
                endColumn: word.endColumn,
            };
            const suggestions = Instructions.map((inst) => ({
                label: inst,
                kind: monaco.languages.CompletionItemKind.Text,
                insertText: inst,
                range: range,
            }))
            return { suggestions: suggestions };
        },
    });
}