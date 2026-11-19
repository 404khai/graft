import { AtWork } from "./components/AtWork";
import { Compare } from "./components/Compare";
import { Cta } from "./components/Cta";
import { Features } from "./components/Features";
import { Footer } from "./components/Footer";
import { Hero } from "./components/Hero";
import { Nav } from "./components/Nav";
import { ProductShot } from "./components/ProductShot";

export function App() {
  return (
    <div className="rails">
      <Nav />
      <main>
        <Hero />
        <ProductShot />
        <Features />
        <AtWork />
        <Compare />
        <Cta />
      </main>
      <Footer />
    </div>
  );
}
