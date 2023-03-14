var n="#define EXCEPTION [102]\n#define TRAP 4\n\nmain:\n    trap\n    reset\n\ninvalid:\n    .word 0\n\n.addr 200\nhandler:\n    push %a\n    ld EXCEPTION, %a\n    cmp TRAP, %a\n    jne exit\n    pop %a\n    rti\n\nexit:\n    reset\n";export{n as default};
//# sourceMappingURL=handler-6043523c.js.map
