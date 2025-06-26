from basics import *
from mdk4 import *

def welcome():
    clrscr()
    banner()
    print("\n\n")
    typewriter.blue("Welcome To Death_Pwn")
    typewriter.all_color([("            -Made By 'Hack With Tarang' With ", "bright_cyan"), ("♥", "bright_magenta")], dealay=0.02)
    typewriter.yellow("\n\n\nFor Solving any problems visit- https://t.me/+2kuYPW5ETw8zMjNl", dealay=0.02)
    delay(2)
    clrscr()


def query_filter(query):
    
    if "jam" in query:
        try:
            mdk4(interface=int_face, mode="d").Jammer()
        except KeyboardInterrupt:
            intercept("Jammer")
        except Exception as e:
            typewriter.red_bright("There is Error in jammer in query filter,")
            print(e)
            delay(2)

    elif any(keyword in query for keyword in beacon_dict):
        try:
            mdk4(interface=int_face, mode="b").Beacon_Flooding()
        except KeyboardInterrupt:
            intercept("Beacon Flooding")
        except Exception as e:
            typewriter.red_bright("There is Error in beacon flooding in query filter,")
            print(e)
            delay(2)

    elif "scan" in query:
        try:
            airmon_scan(interface=int_face)
        except Exception as e:
            typewriter.red_bright("There is Error in beacon flooding in query filter,")
            print(e)
            delay(2)
        except KeyboardInterrupt:
            intercept("Wi-fi scan")

    elif ("help" or "-h") in query:
        typewriter.green("You can do jamming.               ♥ makes all wi-fi down", dealay=0.01)
        typewriter.green("You can do beacon flooding.       ♥ makes bunch of fake wi-fi", dealay=0.01)
        typewriter.green("You can scan networks around you. ♥ scans networks around you and save for later\n\n\n", dealay=0.01)
        typewriter.yellow("Other features are under developtment")
        typewriter.yellow_bright(enter)
        input()

    else:
        typewriter.yellow_bright("Not recognized query!")
        typewriter.red(enter)
        input()



def main():

    try:
        while True:
            new_page()
            user_query = u_input().lower()
            
            if "exit" in user_query or "quit" in user_query:
                raise KeyboardInterrupt
                
            query_filter(user_query)
            
    except KeyboardInterrupt:
        typewriter.yellow_bright("\nExiting...")
        mode.ma()
        clrscr()
        sys.exit(0)



if __name__ == "__main__":
    if is_sudo():
        try:
            mode.mo()
            welcome()
            main()
        except KeyboardInterrupt:
            mode.ma()
            clrscr()
            sys.exit(0)
        except Exception as e:
            typewriter.red_bright(f"Fatal error: ")
            print(e)
            input()
            mode.ma()
            sys.exit(1)
    else:
        typewriter.red_bright("Run this script as sudo!")
