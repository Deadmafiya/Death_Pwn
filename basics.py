import os
import subprocess
import sys
from time import sleep as delay
from rich.console import Console
from rich.prompt import Prompt
from rich.panel import Panel

console = Console()

#vars
complete = "[green]Task [bold bright_green]completed![/bold bright_green] press [bold bright_green]Enter[/bold bright_green] to return.[/green]"
input_text = "Death_Pwn#~"
int_face = "wlan1"
beacon_dict = [
    "pakets", 
    "paket",
    "pakte",
    "paktes",
    "packte",
    "packtes",
    "packet",
    "packets", 
    "peket",
    "pekets",
    "pecket",
    "peckets",
    "pakit",
    "pakkets",
    "pakats",
    "packit",
    "puckets",
    "packeets",
    "pactets",
    "backets", 
    "patkets", 
    "pakez",   
    "paketz",  
    "pakeet",
    "paquets",
    "paakets"
]



def completed():
        Prompt.ask(complete)
        clrscr()

def clrscr():
    os.system('clear' if os.name != 'nt' else 'cls')

def banner():
    console.print(Panel.fit("[bold cyan]DEATH Pwn[/bold cyan]\n[magenta]By Dead Mafia[/magenta]", border_style="cyan"))

def logo(logo_name: str = None):
    console.print(Panel.fit(f"[bold cyan]{logo_name}[/bold cyan]", title_align="center", border_style="cyan"))

def x_logo(logo_name: str):
    clrscr()
    banner()
    logo(logo_name)

def new_page():
    clrscr()
    banner()


class mode:
    def mo():
        output = subprocess.getoutput('iwconfig')

        if "wlan1" in output:
            clrscr()
            console.print("[bold yellow]Starting WLAN1 in monitor mode![/bold yellow]")
            subprocess.run(["sudo", "airmon-ng", "start", "wlan1"])
            console.print("[bold yellow]monitor mode enabled![/bold yellow]")
            delay(1)
            clrscr()
        else:
            console.print("[bold red]Connect wlan1 first[/bold red]")

    def ma():
        if int_face == "wlan0":
            clrscr()
            console.print("[bold yellow]Starting WLAN1 in monitor mode![/bold yellow]")
            subprocess.run(["sudo", "airmon-ng", "stop", "wlan0mon"])
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


    def green(text, dealay=0.05, style="bold bright_green"):
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


    def yellow(text, dealay=0.05, style="bold bright_yellow"):
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