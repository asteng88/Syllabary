# What we found in C:\lotus\wordpro\wordpro.exe

wordpro.exe is a program for Windows, built for x86 (32-bit). Its internal names were removed (stripped), so most functions will have generated names.

Evidence ev_e8254f1a7aa3c7d9978c4645f409ef1ebc9e6e1d344b930e95803ea64df5a0f7.

```json
{
  "architecture": "x86",
  "bits": 32,
  "dotnet_arch": null,
  "entry_point": "0x4a90b0",
  "format": "pe",
  "format_name": "PE (Windows)",
  "kind": "executable",
  "libraries": [
    "ADVAPI32.dll",
    "GDI32.dll",
    "KERNEL32.dll",
    "MSVCRT.dll",
    "SHELL32.dll",
    "USER32.dll",
    "WINSPOOL.DRV",
    "comdlg32.dll"
  ],
  "next_steps": [
    {
      "tool": "core_overview_show",
      "why": "a one-page summary: sections, imports, counts"
    },
    {
      "tool": "native_functions_list",
      "why": "every function we found"
    },
    {
      "tool": "native_strings_find",
      "why": "find text the program shows or uses"
    },
    {
      "tool": "native_function_study",
      "why": "a full dossier on one function (start with main or the entry point)"
    },
    {
      "tool": "core_feature_find",
      "why": "follow a word you see in the app to the code behind it"
    },
    {
      "tool": "native_pe_resources_list",
      "why": "icons, version information and the manifest"
    }
  ],
  "operating_system": "Windows",
  "plain_english": "wordpro.exe is a program for Windows, built for x86 (32-bit). Its internal names were removed (stripped), so most functions will have generated names.",
  "recommended_toolsets": [
    "core",
    "native",
    "formats"
  ],
  "stripped": true,
  "symbol_table_stripped": true,
  "toolchain_hints": [
    {
      "category": "compiler",
      "evidence": "Rich header",
      "id": "msvc",
      "label": "Microsoft Visual C++ toolchain",
      "strength": "strong"
    }
  ]
}
```

## Ghidra

Deep mode: Ghidra has analysed C:\lotus\wordpro\wordpro.exe (3 min), so decompiling it is quick.

## How it looks running

capture/window.png is a picture of its window 15 seconds after it started. Open it with the Read tool: match its layout, colours and proportions.
capture/ui-tree.json lists the controls in its windows at that moment, in order (role, title, identifier, value, position and size in pixels): use it for the names, order and placement of its menus, buttons and fields.
