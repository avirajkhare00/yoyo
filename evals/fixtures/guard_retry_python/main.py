def greet():
    return "hi"


def banner():
    return f"banner: {greet()}"


if __name__ == "__main__":
    print(banner())
