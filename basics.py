import os
import subprocess
import sys
from time import sleep as delay
from rich.console import Console
from rich.prompt import Prompt
from rich.panel import Panel
from rich.text import Text

console = Console()

#vars
completed = " is completed!"
input_text = "[bold bright_yellow]Death_Pwn#~[/bold bright_yellow]"
enter = "Press any key to continue!"
intercepted = " is Intercepted!"
int_face = "wlan0"
beacon_dict = [
    "beacon",
    "beacon flooding",
    "flooding",
    "floding",
    "flud",
    "spam",
    "fake",
    "becon",
    "bekon",
    "bacon",
    "bakon",
    "bicon",
    "bikon",
    "fluid"
]


def is_sudo():
    if os.geteuid() == 0:
        return True
    else:
        return False
    
def clrscr():
    os.system('clear' if os.name != 'nt' else 'cls')

def u_input():
    u_input = Prompt.ask(input_text)
    return u_input

def complete(task):
        typewriter.green_bright(task+completed)
        typewriter.green(enter)
        input()
        clrscr()

def intercept(task):
        typewriter.red_bright(task+intercepted)
        typewriter.red(enter)
        input()
        clrscr()

def banner():
    def rgb_gradient(start_rgb, end_rgb, steps):
        """Generate gradient colors from start to end RGB in given steps."""
        return [
            (
                int(start_rgb[0] + (end_rgb[0] - start_rgb[0]) * i / (steps - 1)),
                int(start_rgb[1] + (end_rgb[1] - start_rgb[1]) * i / (steps - 1)),
                int(start_rgb[2] + (end_rgb[2] - start_rgb[2]) * i / (steps - 1)),
            )
            for i in range(steps)
        ]

    ascii_lines = [
    "███████╗ ███████╗ █████╗ ████████╗██╗  ██╗    ██████╗ ██╗    ██╗███╗   ██╗",
    " ██╔══██╗██╔════╝██╔══██╗╚══██╔══╝██║  ██║    ██╔══██╗██║    ██║████╗  ██║",
    " ██║  ██║█████╗  ███████║   ██║   ███████║    ██████╔╝██║ █╗ ██║██╔██╗ ██║",
    " ██║  ██║██╔══╝  ██╔══██║   ██║   ██╔══██║    ██╔═══╝ ██║███╗██║██║╚██╗██║",
    "███████╔╝███████╗██║  ██║   ██║   ██║  ██║    ██║     ╚███╔███╔╝██║ ╚████║",
    " ╚═════╝ ╚══════╝╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝    ╚═╝      ╚══╝╚══╝ ╚═╝  ╚═══╝"
    ]

    start_color = (221, 160, 221)
    end_color = (75, 0, 130)

    gradient = rgb_gradient(start_color, end_color, len(ascii_lines))

    for i, line in enumerate(ascii_lines):
        r, g, b = gradient[i]
        hex_color = f"#{r:02x}{g:02x}{b:02x}"
        text = Text(line, style=hex_color)
        console.print(text)

def new_page():
    clrscr()
    banner()

class mode:
    def mo():
        output = subprocess.getoutput('iwconfig')

        if int_face == "wlan0":
            clrscr()
            console.print("[bold yellow]Starting WLAN1 in monitor mode![/bold yellow]")
            subprocess.run(["sudo", "airmon-ng", "start", "wlan0"])
            console.print("[bold yellow]monitor mode enabled![/bold yellow]")
            delay(1)
            clrscr()
        else:
            console.print("[bold red]Connect wlan1 first[/bold red]")

    def ma():
        if int_face == "wlan0":
            clrscr()
            console.print("[bold yellow]Starting WLAN1 in monitor mode![/bold yellow]")
            subprocess.run(["sudo", "airmon-ng", "stop", "wlan0"])
            console.print("[bold yellow]managed mode enabled![/bold yellow]")
            delay(1)
            clrscr()
        else:
            clrscr()
            console.print("[bold yellow]Starting WLAN1 in monitor mode![/bold yellow]")
            subprocess.run(["sudo", "airmon-ng", "stop", "wlan1"])
            console.print("[bold yellow]managed mode enabled![/bold yellow]")
            delay(1)
            clrscr()

class typewriter:
    def all_color(segments, dealay=0.05):
        """
        segments: List of tuples -> (text, style)
                Example: [("Hello", "bold red"), (" World", "italic bright_blue")]
        """
        for text, style in segments:
            with console.capture() as capture:
                console.print(text, style=style, end="")
            styled_text = capture.get()
            
            for char in styled_text:
                sys.stdout.write(char)
                sys.stdout.flush()
                delay(dealay)
        print()

    def green(text, dealay=0.05, style="green"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def green_bright(text, dealay=0.05, style="bold bright_green"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def red_bright(text, dealay=0.05, style="bold bright_red"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def red(text, dealay=0.05, style="red"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def blue(text, dealay=0.05, style="bold bright_blue"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def yellow(text, dealay=0.05, style="yellow"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def yellow_bright(text, dealay=0.05, style="bold bright_yellow"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def cyan_bright(text, dealay=0.05, style="bold bright_cyan"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def cyan(text, dealay=0.05, style="cyan"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()

    def magenta_bright(text, dealay=0.05, style="bold bright_magenta"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()
    
    def magenta(text, dealay=0.05, style="magenta"):
        with console.capture() as capture:
            console.print(text, style=style, end="")
        styled_text = capture.get()
        
        for char in styled_text:
            sys.stdout.write(char)
            sys.stdout.flush()
            delay(dealay)
        print()
